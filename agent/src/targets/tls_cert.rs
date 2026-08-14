use std::{
    fmt::Display,
    net::IpAddr,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

use chrono::{DateTime, Utc};
use rustls::{
    ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    crypto::CryptoProvider,
    pki_types::{CertificateDer, ServerName, UnixTime, pem::PemObject},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpSocket, lookup_host};
use tokio_rustls::TlsConnector;
use tracing_batteries::prelude::opentelemetry::trace::SpanKind as OpenTelemetrySpanKind;
use tracing_batteries::prelude::*;
use x509_parser::prelude::*;

use crate::{Sample, Target};

lazy_static! {
    /// Both the `ring` and `aws-lc-rs` backends are enabled somewhere in the dependency
    /// graph, so rustls has no unambiguous default provider and one must be named here.
    static ref PROVIDER: Arc<CryptoProvider> = Arc::new(rustls::crypto::ring::default_provider());
    static ref NATIVE_ROOTS: Arc<RootCertStore> = {
        let mut roots = RootCertStore::empty();
        let loaded = rustls_native_certs::load_native_certs();
        for cert in loaded.certs {
            let _ = roots.add(cert);
        }

        for error in loaded.errors {
            warn!("Failed to load a system CA certificate: {error}");
        }

        Arc::new(roots)
    };
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TlsCertTarget {
    pub host: String,
    #[serde(default)]
    pub server_name: Option<String>,
    #[serde(default)]
    pub ca_cert: Option<String>,
    #[serde(default)]
    pub alpn: Vec<String>,
}

impl TlsCertTarget {
    fn server_name(&self) -> &str {
        self.server_name.as_deref().unwrap_or(host_of(&self.host))
    }

    fn roots(&self) -> Result<Arc<RootCertStore>, Box<dyn std::error::Error>> {
        let Some(pem) = &self.ca_cert else {
            return Ok(NATIVE_ROOTS.clone());
        };

        let mut roots = RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(pem.as_bytes()) {
            roots.add(cert?)?;
        }

        if roots.is_empty() {
            return Err(
                "The provided 'ca_cert' did not contain any PEM encoded certificates.".into(),
            );
        }

        Ok(Arc::new(roots))
    }
}

impl Target for TlsCertTarget {
    #[tracing::instrument(
        "target.tls_cert",
        skip(self, _cancel), err(Debug),
        fields(
            otel.kind=?OpenTelemetrySpanKind::Client,
            tls.host = %self.host,
            tls.server_name = %self.server_name(),
            tls.version = EmptyField,
            tls.trusted = EmptyField,
            tls.not_after = EmptyField,
    ))]
    async fn run(&self, _cancel: &AtomicBool) -> Result<Sample, Box<dyn std::error::Error>> {
        let server_name = ServerName::try_from(self.server_name().to_string())?;

        let verifier = CapturingVerifier::new(
            WebPkiServerVerifier::builder_with_provider(self.roots()?, PROVIDER.clone()).build()?,
        );
        let capture = verifier.capture.clone();

        let mut config = ClientConfig::builder_with_provider(PROVIDER.clone())
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        config.alpn_protocols = self.alpn.iter().map(|p| p.as_bytes().to_vec()).collect();

        let addr = lookup_host(&self.host)
            .await?
            .next()
            .ok_or(format!("Could not resolve the hostname '{}'.", self.host))?;

        let sock = if addr.is_ipv4() {
            TcpSocket::new_v4()?
        } else {
            TcpSocket::new_v6()?
        };

        let mut stream = TlsConnector::from(Arc::new(config))
            .connect(server_name, sock.connect(addr).await?)
            .await?;

        let connection = stream.get_ref().1;
        let captured = capture
            .lock()
            .expect("the certificate capture mutex is never held across a panic")
            .take()
            .ok_or("The server did not present a TLS certificate.")?;

        let sample = Sample::default()
            .with("net.ip", addr.ip().to_string())
            .with(
                "tls.version",
                connection
                    .protocol_version()
                    .map(|v| format!("{v:?}").replace('_', ".")),
            )
            .with(
                "tls.cipher_suite",
                connection
                    .negotiated_cipher_suite()
                    .map(|s| format!("{:?}", s.suite())),
            )
            .with(
                "tls.alpn",
                connection
                    .alpn_protocol()
                    .map(|p| String::from_utf8_lossy(p).into_owned()),
            );

        // Send `close_notify` so the server can release the connection immediately
        // rather than waiting for it to time out. The probe has everything it needs
        // by this point, so a peer which has already hung up is not a failure.
        if let Err(err) = stream.shutdown().await {
            debug!(
                "Failed to cleanly close the connection to '{}': {err}",
                self.host
            );
        }

        captured.describe(sample, self.server_name())
    }
}

impl Display for TlsCertTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.server_name {
            Some(name) => write!(f, "TLS {} ({})", self.host, name),
            None => write!(f, "TLS {}", self.host),
        }
    }
}

/// The certificate chain a server presented, together with the verdict the real
/// verifier reached on it.
#[derive(Debug)]
struct CapturedChain {
    leaf: CertificateDer<'static>,
    intermediates: Vec<CertificateDer<'static>>,
    verification: Result<(), rustls::Error>,
}

impl CapturedChain {
    /// Adds the certificate's details to `sample`, deriving expiry and hostname
    /// validity from the certificate itself so that each is independently
    /// assertable rather than collapsed into a single verification error.
    fn describe(
        &self,
        sample: Sample,
        server_name: &str,
    ) -> Result<Sample, Box<dyn std::error::Error>> {
        let (_, leaf) = X509Certificate::from_der(&self.leaf)?;

        let not_before = asn1_to_datetime(leaf.validity().not_before)?;
        let not_after = asn1_to_datetime(leaf.validity().not_after)?;
        let now = Utc::now();
        let sans = subject_alternative_names(&leaf);

        let mut chain_subjects = vec![leaf.subject().to_string()];
        let mut chain_issuers = vec![leaf.issuer().to_string()];
        for intermediate in &self.intermediates {
            let (_, cert) = X509Certificate::from_der(intermediate)?;
            chain_subjects.push(cert.subject().to_string());
            chain_issuers.push(cert.issuer().to_string());
        }

        Ok(sample
            .with("tls.trusted", self.verification.is_ok())
            .with(
                "tls.error",
                self.verification.as_ref().err().map(|e| e.to_string()),
            )
            .with(
                "tls.hostname_valid",
                matches_hostname(&sans, common_name(leaf.subject()).as_deref(), server_name),
            )
            .with("tls.expired", now < not_before || now > not_after)
            .with("tls.not_before", not_before)
            .with("tls.not_after", not_after)
            .with("tls.expires_in", not_after - now)
            .with("tls.subject", leaf.subject().to_string())
            .with("tls.subject_cn", common_name(leaf.subject()))
            .with("tls.issuer", leaf.issuer().to_string())
            .with("tls.issuer_cn", common_name(leaf.issuer()))
            .with("tls.serial", hex::encode(leaf.raw_serial()))
            .with("tls.thumbprint", hex::encode(Sha256::digest(&self.leaf)))
            .with("tls.signature_algorithm", signature_algorithm(&leaf))
            .with("tls.sans", sans)
            .with("tls.chain.length", chain_subjects.len() as i64)
            .with("tls.chain.subjects", chain_subjects)
            .with("tls.chain.issuers", chain_issuers))
    }
}

/// A [`ServerCertVerifier`] which records what the server presented and what the
/// real verifier made of it, but always reports success so that the handshake
/// completes and the certificate's details can be reported as probe fields.
#[derive(Debug)]
struct CapturingVerifier {
    inner: Arc<WebPkiServerVerifier>,
    capture: Arc<Mutex<Option<CapturedChain>>>,
}

impl CapturingVerifier {
    fn new(inner: Arc<WebPkiServerVerifier>) -> Self {
        Self {
            inner,
            capture: Arc::new(Mutex::new(None)),
        }
    }
}

impl ServerCertVerifier for CapturingVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let verification = self
            .inner
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
            .map(|_| ());

        if let Ok(mut capture) = self.capture.lock() {
            *capture = Some(CapturedChain {
                leaf: end_entity.clone().into_owned(),
                intermediates: intermediates
                    .iter()
                    .map(|c| c.clone().into_owned())
                    .collect(),
                verification,
            });
        }

        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// Strips the port (and any IPv6 brackets) from a `host:port` pair.
fn host_of(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }

    host.rsplit_once(':').map(|(host, _)| host).unwrap_or(host)
}

fn common_name(name: &X509Name<'_>) -> Option<String> {
    name.iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .map(|cn| cn.to_string())
}

fn subject_alternative_names(cert: &X509Certificate<'_>) -> Vec<String> {
    let Ok(Some(san)) = cert.subject_alternative_name() else {
        return Vec::new();
    };

    san.value
        .general_names
        .iter()
        .filter_map(|name| match name {
            GeneralName::DNSName(name) => Some((*name).to_string()),
            GeneralName::IPAddress(bytes) => match bytes.len() {
                4 => <[u8; 4]>::try_from(*bytes)
                    .ok()
                    .map(|b| IpAddr::from(b).to_string()),
                16 => <[u8; 16]>::try_from(*bytes)
                    .ok()
                    .map(|b| IpAddr::from(b).to_string()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn signature_algorithm(cert: &X509Certificate<'_>) -> String {
    let oid = cert.signature_algorithm.oid();
    x509_parser::objects::oid2sn(oid, x509_parser::objects::oid_registry())
        .map(|sn| sn.to_string())
        .unwrap_or_else(|_| oid.to_id_string())
}

fn asn1_to_datetime(time: ASN1Time) -> Result<DateTime<Utc>, Box<dyn std::error::Error>> {
    DateTime::from_timestamp(time.timestamp(), 0).ok_or_else(|| {
        format!("The certificate contained an out-of-range timestamp '{time}'.").into()
    })
}

/// Reports whether the certificate is valid for `server_name`, applying the usual
/// rule that a wildcard matches exactly one leading label. Falls back to the
/// common name only when the certificate carries no subject alternative names.
fn matches_hostname(sans: &[String], common_name: Option<&str>, server_name: &str) -> bool {
    let candidates: Vec<&str> = if sans.is_empty() {
        common_name.into_iter().collect()
    } else {
        sans.iter().map(String::as_str).collect()
    };

    candidates
        .iter()
        .any(|candidate| matches_name(candidate, server_name))
}

fn matches_name(candidate: &str, server_name: &str) -> bool {
    let Some(suffix) = candidate.strip_prefix("*.") else {
        return candidate.eq_ignore_ascii_case(server_name);
    };

    match server_name.split_once('.') {
        Some((_, rest)) => rest.eq_ignore_ascii_case(suffix),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SampleValue;
    use rustls::pki_types::PrivateKeyDer;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    /// Serves a single TLS connection using a freshly minted self-signed
    /// certificate, returning the address to probe, the CA PEM which vouches for
    /// it, the exact (second-precision) expiry that was baked into it, and a
    /// handle reporting whether the client closed the connection gracefully.
    async fn serve_self_signed(
        expires_in: chrono::Duration,
    ) -> (String, String, DateTime<Utc>, JoinHandle<bool>) {
        let not_after = DateTime::from_timestamp((Utc::now() + expires_in).timestamp(), 0)
            .expect("build an in-range expiry");

        let mut params =
            rcgen::CertificateParams::new(vec!["localhost".to_string(), "*.localhost".to_string()])
                .expect("build certificate parameters");
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "grey-test");
        params.not_before = rcgen::date_time_ymd(2020, 1, 1);
        params.not_after = ::time::OffsetDateTime::from_unix_timestamp(not_after.timestamp())
            .expect("convert the expiry");

        let key = rcgen::KeyPair::generate().expect("generate key pair");
        let cert = params.self_signed(&key).expect("sign certificate");
        let pem = cert.pem();

        let server = rustls::ServerConfig::builder_with_provider(PROVIDER.clone())
            .with_safe_default_protocol_versions()
            .expect("select protocol versions")
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivateKeyDer::try_from(key.serialize_der()).expect("encode private key"),
            )
            .expect("build server config");

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("read local address");

        // Reading past the end of the client's data distinguishes a graceful
        // close (`close_notify`, surfacing as EOF) from a dropped connection.
        let closed_cleanly = tokio::spawn(async move {
            let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
            let Ok((stream, _)) = listener.accept().await else {
                return false;
            };
            let Ok(mut stream) = acceptor.accept(stream).await else {
                return false;
            };

            matches!(stream.read(&mut [0u8; 1]).await, Ok(0))
        });

        (
            format!("127.0.0.1:{}", addr.port()),
            pem,
            not_after,
            closed_cleanly,
        )
    }

    fn target(host: &str, ca_cert: Option<String>) -> TlsCertTarget {
        TlsCertTarget {
            host: host.to_string(),
            server_name: Some("localhost".to_string()),
            ca_cert,
            alpn: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_reports_certificate_details() {
        let (host, pem, expiry, closed_cleanly) =
            serve_self_signed(chrono::Duration::days(60)).await;

        let sample = target(&host, Some(pem))
            .run(&AtomicBool::new(false))
            .await
            .expect("probe the test server");

        assert_eq!(sample.get("tls.trusted"), &SampleValue::Bool(true));
        assert_eq!(sample.get("tls.error"), &SampleValue::None);
        assert_eq!(sample.get("tls.hostname_valid"), &SampleValue::Bool(true));
        assert_eq!(sample.get("tls.expired"), &SampleValue::Bool(false));
        assert_eq!(
            sample.get("tls.subject_cn"),
            &SampleValue::from("grey-test")
        );
        assert_eq!(sample.get("tls.chain.length"), &SampleValue::from(1));
        assert_eq!(
            sample.get("tls.sans"),
            &SampleValue::from(vec!["localhost", "*.localhost"])
        );
        assert_eq!(sample.get("tls.version"), &SampleValue::from("TLSv1.3"));
        assert!(
            matches!(sample.get("tls.thumbprint"), SampleValue::String(s) if s.len() == 64),
            "expected a hex-encoded SHA-256 thumbprint, got {}",
            sample.get("tls.thumbprint")
        );

        // The expiry is exposed both absolutely and relatively, so checks can be
        // written against `now()` or against a duration literal such as `30d`.
        assert!(matches!(sample.get("tls.not_after"), SampleValue::DateTime(d) if *d == expiry));
        assert!(
            matches!(sample.get("tls.expires_in"), SampleValue::Duration(d) if *d > chrono::Duration::days(59))
        );

        assert!(
            closed_cleanly.await.expect("the test server should finish"),
            "the probe should send close_notify so the server can release the connection"
        );
    }

    /// An untrusted certificate must still yield a full sample, so that a probe
    /// can alert on the loss of trust rather than simply erroring out.
    #[tokio::test]
    async fn test_untrusted_certificate_still_reports_details() {
        let (host, _, _, _) = serve_self_signed(chrono::Duration::days(60)).await;

        let sample = target(&host, None)
            .run(&AtomicBool::new(false))
            .await
            .expect("probe the test server");

        assert_eq!(sample.get("tls.trusted"), &SampleValue::Bool(false));
        assert!(matches!(sample.get("tls.error"), SampleValue::String(e) if !e.is_empty()));
        assert_eq!(sample.get("tls.hostname_valid"), &SampleValue::Bool(true));
        assert_eq!(
            sample.get("tls.subject_cn"),
            &SampleValue::from("grey-test")
        );
    }

    #[tokio::test]
    async fn test_expired_certificate_is_reported() {
        let (host, pem, _, _) = serve_self_signed(chrono::Duration::days(-1)).await;

        let sample = target(&host, Some(pem))
            .run(&AtomicBool::new(false))
            .await
            .expect("probe the test server");

        assert_eq!(sample.get("tls.expired"), &SampleValue::Bool(true));
        assert_eq!(sample.get("tls.trusted"), &SampleValue::Bool(false));
        assert!(
            matches!(sample.get("tls.expires_in"), SampleValue::Duration(d) if d.num_seconds() < 0)
        );
    }

    #[test]
    fn test_display() {
        let target = TlsCertTarget {
            host: "example.com:443".to_string(),
            server_name: None,
            ca_cert: None,
            alpn: Vec::new(),
        };
        assert_eq!(target.to_string(), "TLS example.com:443");

        let target = TlsCertTarget {
            server_name: Some("internal.example.com".to_string()),
            ..target
        };
        assert_eq!(
            target.to_string(),
            "TLS example.com:443 (internal.example.com)"
        );
    }

    #[test]
    fn test_host_of() {
        assert_eq!(host_of("example.com:443"), "example.com");
        assert_eq!(host_of("example.com"), "example.com");
        assert_eq!(host_of("127.0.0.1:443"), "127.0.0.1");
        assert_eq!(host_of("[::1]:443"), "::1");
    }

    #[test]
    fn test_matches_hostname() {
        let sans = vec!["example.com".to_string(), "*.example.com".to_string()];

        assert!(matches_hostname(&sans, None, "example.com"));
        assert!(matches_hostname(&sans, None, "EXAMPLE.COM"));
        assert!(matches_hostname(&sans, None, "api.example.com"));
        assert!(!matches_hostname(&sans, None, "a.b.example.com"));
        assert!(!matches_hostname(&sans, None, "example.org"));

        // The common name is only consulted when there are no SANs at all.
        assert!(matches_hostname(&[], Some("example.com"), "example.com"));
        assert!(!matches_hostname(&sans, Some("example.org"), "example.org"));
    }

    /// Exercises a real, publicly trusted endpoint, which is the only way to
    /// cover chain building against the system trust store and a server which
    /// sends intermediates.
    #[tokio::test]
    #[cfg(not(feature = "pure_tests"))]
    async fn test_public_endpoint() {
        let target = TlsCertTarget {
            host: "sierrasoftworks.com:443".to_string(),
            server_name: None,
            ca_cert: None,
            alpn: vec!["h2".to_string(), "http/1.1".to_string()],
        };

        let sample = target
            .run(&AtomicBool::new(false))
            .await
            .expect("probe a public endpoint");

        assert_eq!(sample.get("tls.trusted"), &SampleValue::Bool(true));
        assert_eq!(sample.get("tls.hostname_valid"), &SampleValue::Bool(true));
        assert_eq!(sample.get("tls.expired"), &SampleValue::Bool(false));
        assert!(
            matches!(sample.get("tls.chain.length"), SampleValue::Int(n) if *n > 1),
            "a public endpoint should send its intermediates, got {}",
            sample.get("tls.chain.length")
        );
        assert!(matches!(sample.get("tls.expires_in"), SampleValue::Duration(d) if !d.is_zero()));
        assert!(matches!(sample.get("tls.alpn"), SampleValue::String(_)));
    }
}
