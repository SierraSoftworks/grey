use deno_core::anyhow;
use serde::{Deserialize, Serialize};
use std::{fmt::Display, sync::atomic::AtomicBool};

use crate::{targets::Target, Sample};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ScriptTarget {
    pub code: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[async_trait::async_trait]
impl Target for ScriptTarget {
    async fn run(&self, _cancel: &AtomicBool) -> Result<Sample, Box<dyn std::error::Error>> {
        let code = self.code.clone();
        let args = self.args.clone();

        let (send, recv) = tokio::sync::oneshot::channel::<Result<Sample, anyhow::Error>>();

        tokio::task::spawn_blocking(move || {
            std::thread::spawn(move || {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => {
                        let result =
                            rt.block_on(
                                async move { crate::deno::run_probe_script(&code, args).await },
                            );

                        send.send(result).ok();
                    }
                    Err(err) => {
                        send.send(Err(err.into())).ok();
                    }
                }
            });
        });

        recv.await?.map_err(|e| format!("{e}").into())
    }
}

impl Display for ScriptTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "probe.script({})",
            self.args
                .iter()
                .map(|a| serde_json::to_string(a).unwrap_or_else(|_| format!("\"{a}\"")))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::sample::SampleValue;

    use super::*;

    #[tokio::test]
    async fn test_script_ok() {
        let target = ScriptTarget {
            code: "console.log('Hello, world!');".to_string(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);

        target.run(&cancel).await.expect("no error to be raised");
    }

    #[tokio::test]
    async fn test_script_invalid_syntax() {
        let target = ScriptTarget {
            code: "console.log('Hello, world!".to_string(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);

        target
            .run(&cancel)
            .await
            .expect_err("an error should be raised");
    }

    #[tokio::test]
    async fn test_script_with_outputs() {
        let target = ScriptTarget {
            code: r#"setOutput('abc', '123'); setOutput('x', 'y')"#.into(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);

        let sample = target.run(&cancel).await.expect("no error to be raised");
        assert_eq!(sample.get("abc"), &SampleValue::from("123"));
    }

    #[tokio::test]
    async fn test_script_fetch() {
        deno_runtime::deno_tls::rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("Failed to initialize crypto subsystem");

        let target = ScriptTarget {
            code: r#"
            const result = await fetch("https://bender.sierrasoftworks.com/api/v1/quote/bender", {
                headers: getTraceHeaders()
            });
            setOutput('http.status_code', result.status);
            const quote = await result.json();
            setOutput('quote.who', quote.who);
            "#
            .into(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);

        let sample = target.run(&cancel).await.expect("no error to be raised");
        assert_eq!(sample.get("http.status_code"), &SampleValue::from(200));
        assert_eq!(sample.get("quote.who"), &SampleValue::from("Bender"));
    }
}
