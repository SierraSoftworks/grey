use super::*;
use std::{hash::Hash, net::SocketAddr, str::FromStr, sync::Arc};
use serde::{Serialize, de::DeserializeOwned};
use tokio::net::UdpSocket;
use crate::cluster::transport::encryption::{EncryptionKeyProvider, EncryptionProvider};

/// Largest UDP datagram payload we will ever receive (IPv4: 65535 - 20-byte IP header - 8-byte UDP
/// header). Used to size the receive buffer; the per-message send limit is configurable.
const MAX_DATAGRAM_SIZE: usize = 65507;

pub struct UdpGossipTransport<E, K>
where
    E: EncryptionProvider,
    K: EncryptionKeyProvider<Key = E::Key>,
{
    socket: Arc<UdpSocket>,
    encryption_provider: E,
    key_provider: K,
    /// Maximum size, in bytes, of an encrypted datagram this transport will emit. Larger messages
    /// are partitioned across rounds to fit. Lower it below the path MTU to avoid IP fragmentation.
    max_message_size: usize,
}

impl<E, K> UdpGossipTransport<E, K>
where
    E: EncryptionProvider,
    K: EncryptionKeyProvider<Key = E::Key>,
{
    pub async fn new(
        addr: &str,
        max_message_size: usize,
        encryption_provider: E,
        key_provider: K,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let addr = SocketAddr::from_str(addr)?;
        let socket = UdpSocket::bind(addr).await?;

        Ok(Self {
            socket: Arc::new(socket),
            encryption_provider,
            key_provider,
            max_message_size,
        })
    }
}

impl <E, K, P, T> GossipTransport<P, T> for UdpGossipTransport<E, K>
where
    E: EncryptionProvider,
    K: EncryptionKeyProvider<Key = E::Key>,
    P: Eq + Hash + Clone + Serialize + DeserializeOwned + Send + 'static,
    T: Versioned + Serialize + DeserializeOwned + Send + 'static,
{
    type Address = SocketAddr;

    async fn send(
        &self,
        address: Self::Address,
        msg: Message<P, T>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut msg = msg;
        loop {
            let data = rmp_serde::to_vec(&msg)?;
            let encrypted = self.encryption_provider.encrypt(&self.key_provider, &data)?;

            // Send once the encrypted datagram fits the frame, or once the message has been reduced
            // to its digest and cannot be partitioned further. The latter is best effort: `send_to`
            // surfaces any oversize error (e.g. a digest larger than the frame) to the caller.
            if encrypted.len() <= self.max_message_size || msg.is_empty() {
                self.socket.send_to(&encrypted, address).await?;
                return Ok(());
            }

            // Estimate how many of the current entries will fit from the measured oversize ratio.
            // Integer division undershoots, which we prefer: gossip is frequent, so sending a few
            // fewer entries now is cheaper than extra serialization passes chasing the exact
            // maximum. Re-measuring each pass lets the estimate self-correct for fixed overhead
            // (metadata, digest) so it converges in one or two iterations.
            let items = msg.len();
            let keep = (items.saturating_mul(self.max_message_size) / encrypted.len()).min(items - 1);
            msg = msg.partition(keep);
        }
    }

    async fn try_receive(
        &self,
    ) -> Result<Option<(Self::Address, Message<P, T>)>, Box<dyn std::error::Error>>
    {
        let mut buf = [0; MAX_DATAGRAM_SIZE];
        // Await the next datagram rather than polling with `try_recv_from`; this removes the
        // ~100 wakeups/s-per-node busy-poll and the up-to-10ms hop latency it incurred.
        let (size, addr) = self.socket.recv_from(&mut buf).await?;
        let decrypted_data = self.encryption_provider.decrypt(&self.key_provider, &buf[..size])?;
        let msg: Message<P, T> = rmp_serde::from_slice(&decrypted_data)?;
        Ok(Some((addr, msg)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngExt;
    use std::net::SocketAddr;
    use tokio::time::{sleep, timeout, Duration};
    use crate::cluster::message::{Message, ClusterStateDigest};
    use crate::cluster::versioned::LastWriteWinsValue;
    use serde::{Serialize, Deserialize};
    use crate::cluster::transport::encryption::{Aes256Gcm, StaticKeyProvider};

    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    struct TestPeer(String);

    fn random_local_addr() -> (String, SocketAddr) {
        let mut rng = rand::rng();
        let port: u16 = rng.random_range(30000..60000);
        let addr_str = format!("127.0.0.1:{}", port);
        let addr = addr_str.parse().unwrap();
        (addr_str, addr)
    }

    #[tokio::test]
    async fn send_partitions_oversized_message_keeping_oldest() {
        let key_provider = StaticKeyProvider::new([7u8; 32]);
        let (addr1_str, _addr1) = random_local_addr();
        let (addr2_str, addr2) = random_local_addr();

        // A deliberately small frame so a modest message must be partitioned to fit.
        let frame = 300usize;
        let sender = UdpGossipTransport::new(&addr1_str, frame, Aes256Gcm, key_provider.clone()).await.unwrap();
        let receiver = UdpGossipTransport::new(&addr2_str, MAX_DATAGRAM_SIZE, Aes256Gcm, key_provider).await.unwrap();

        let peer = TestPeer("p".to_string());
        let total = 50u64;
        let mut diff = ClusterStateDiff::new();
        for v in 0..total {
            diff.update(peer.clone(), format!("field-{v:03}"), LastWriteWinsValue::new(v as i32).with_version(v));
        }
        let msg = Message::<TestPeer, LastWriteWinsValue<i32>>::Ack(MessageMetadata::new(peer.clone()), diff);
        assert_eq!(msg.len(), total as usize);

        sender.send(addr2, msg).await.unwrap();

        let (_src, received): (SocketAddr, Message<TestPeer, LastWriteWinsValue<i32>>) =
            timeout(Duration::from_secs(1), async {
                loop {
                    if let Some(x) = receiver.try_receive().await.unwrap() {
                        break x;
                    }
                    tokio::task::yield_now().await;
                }
            }).await.expect("timed out waiting for partitioned message");

        match received {
            Message::Ack(_, diff) => {
                let len = diff.len();
                assert!(len > 0 && len < total as usize, "message must be partitioned (got {len} of {total})");
                let inner = diff.into_inner();
                let fields = &inner[&peer];
                assert!(fields.contains_key("field-000"), "the oldest entry must be kept");
                assert!(
                    !fields.contains_key(&format!("field-{:03}", total - 1)),
                    "the newest entry must be dropped and re-sent later"
                );
            }
            other => panic!("expected Ack, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn udp_gossip_transport_send_and_receive() {
        let shared_secret = [42u8; 32];
        let key_provider = StaticKeyProvider::new(shared_secret);

        let (addr1_str, addr1) = random_local_addr();
        let (addr2_str, addr2) = random_local_addr();

        let transport1 = UdpGossipTransport::new(&addr1_str, MAX_DATAGRAM_SIZE, Aes256Gcm, key_provider.clone()).await.unwrap();
        let transport2 = UdpGossipTransport::new(&addr2_str, MAX_DATAGRAM_SIZE, Aes256Gcm, key_provider).await.unwrap();

        // Build a simple Syn message
        let peer1 = TestPeer("peer1".to_string());

        let peer1_digest = ClusterStateDigest::new().with_max_version(peer1.clone(), 1);
        let peer1_diff = ClusterStateDiff::new().with_node(peer1.clone(), vec![
            ("key1".to_string(), LastWriteWinsValue::new(1).with_version(1))
        ].into_iter().collect());

        let messages = vec![
            Message::<TestPeer, LastWriteWinsValue<i32>>::Syn(MessageMetadata::new(peer1.clone()), peer1_digest.clone()),
            Message::Ack(MessageMetadata::new(peer1.clone()), peer1_diff.clone()),
            Message::SynAck(MessageMetadata::new(peer1.clone()), peer1_digest.clone(), peer1_diff.clone()),
        ];

        for msg in messages {
            // Send from transport1 to transport2
            transport1.send(addr2, msg).await.unwrap();

            // Try to receive on transport2
            let received = timeout(Duration::from_secs(1), async {
                loop {
                    if let Some((src, m)) = transport2.try_receive().await.unwrap() {
                        break (src, m);
                    }
                    tokio::task::yield_now().await;
                }
            }).await;

            let (src_addr, received_msg) = received.expect("timed out waiting for message");
            match received_msg {
                Message::Syn(meta, d) => {
                    assert_eq!(meta.from, peer1);
                    assert_eq!(&d, &peer1_digest);
                },
                Message::Ack(meta, diff) => {
                    assert_eq!(meta.from, peer1);
                    assert_eq!(&diff, &peer1_diff);
                },
                Message::SynAck(meta, d, diff) => {
                    assert_eq!(meta.from, peer1);
                    assert_eq!(&d, &peer1_digest);
                    assert_eq!(&diff, &peer1_diff);
                },
            }
            assert_eq!(src_addr.ip(), addr1.ip());
        }
    }

    #[tokio::test]
    async fn udp_gossip_transport_wrong_secret_fails() {
        let shared_secret1 = [1u8; 32];
        let shared_secret2 = [2u8; 32];
        let (addr1_str, _addr1) = random_local_addr();
        let (addr2_str, addr2) = random_local_addr();

        let transport1 = UdpGossipTransport::new(&addr1_str, MAX_DATAGRAM_SIZE, Aes256Gcm, StaticKeyProvider::new(shared_secret1)).await.unwrap();
        let transport2 = UdpGossipTransport::new(&addr2_str, MAX_DATAGRAM_SIZE, Aes256Gcm, StaticKeyProvider::new(shared_secret2)).await.unwrap();

        let peer1 = TestPeer("peer1".to_string());
        let mut digest = ClusterStateDigest::new();
        digest.update(peer1.clone(), 1);
        let msg: Message<TestPeer, LastWriteWinsValue<i32>> = Message::Syn(MessageMetadata::new(peer1), digest);

        transport1.send(addr2, msg).await.unwrap();

        // Try to receive on transport2, should fail to decrypt and return Err
        let outcome = timeout(Duration::from_secs(1), async {
            loop {
                match transport2.try_receive().await {
                    Err(_e) => break Err::<(), Box<dyn std::error::Error>>(_e),
                    Ok(Some((.., Message::<TestPeer, LastWriteWinsValue<i32>>::Syn(..)))) => {
                        break Ok::<(), Box<dyn std::error::Error>>(());
                    },
                    Ok(Some(.., msg2)) => {
                        panic!("unexpectedly received message: {:?}", msg2);
                    },
                    Ok(None) => {
                        sleep(Duration::from_millis(10)).await;
                    }
                }
            }
        }).await;

        match outcome {
            Ok(Err(_)) => {}, // expected decryption error
            Ok(Ok(())) => panic!("unexpectedly decrypted message with wrong secret"),
            Err(_) => panic!("timed out waiting for decryption error"),
        }
    }
}