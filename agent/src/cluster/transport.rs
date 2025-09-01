use std::hash::Hash;

use super::*;
#[cfg(test)]
pub use tests::InMemoryGossipTransport;
pub use udp::UdpGossipTransport;

pub trait GossipTransport {
    type Id: Eq + Hash;
    type Address;
    type State: Versioned;

    fn send(
        &self,
        address: Self::Address,
        msg: Message<Self::Id, Self::State>,
    ) -> impl std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>;
    fn try_receive(
        &self,
    ) -> impl std::future::Future<
        Output = Result<
            Option<(Self::Address, Message<Self::Id, Self::State>)>,
            Box<dyn std::error::Error>,
        >,
    >;
}

mod udp {
    use super::*;
    use std::{hash::Hash, net::SocketAddr, str::FromStr, sync::Arc};
    use serde::{Serialize, de::DeserializeOwned};
    use tokio::net::UdpSocket;

    pub struct UdpGossipTransport<P, T> {
        socket: Arc<UdpSocket>,
        shared_secret: [u8; 32],
        _phantom: std::marker::PhantomData<(P, T)>,
    }

    impl<P, T> UdpGossipTransport<P, T> {
        pub async fn new(
            addr: &str,
            shared_secret: [u8; 32],
        ) -> Result<Self, Box<dyn std::error::Error>> {
            let addr = SocketAddr::from_str(addr)?;
            let socket = UdpSocket::bind(addr).await?;

            Ok(Self {
                socket: Arc::new(socket),
                shared_secret,
                _phantom: std::marker::PhantomData,
            })
        }

        fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
            use aes_gcm::aead::{Aead, AeadCore, OsRng};
            use aes_gcm::{Aes256Gcm, Key, KeyInit};

            let key: &Key<Aes256Gcm> = &self.shared_secret.into();
            let cipher = Aes256Gcm::new(&key);

            let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
            let ciphertext = cipher.encrypt(&nonce, plaintext)
                .map_err(|e| format!("Failed to encrypt gossip packet, ensure that you have provided a valid shared secret: {e:?}"))?;

            let mut result = nonce.to_vec();
            result.reserve_exact(ciphertext.len());
            result.extend(ciphertext);
            Ok(result)
        }

        fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
            use aes_gcm::aead::{Aead, Nonce};
            use aes_gcm::{Aes256Gcm, Key, KeyInit};

            if ciphertext.len() < 12 {
                return Err("Ciphertext too short to contain nonce".into());
            }

            let key: &Key<Aes256Gcm> = &self.shared_secret.into();
            let cipher = Aes256Gcm::new(&key);

            let (nonce_bytes, ciphertext) = ciphertext.split_at(12);
            let nonce = Nonce::<Aes256Gcm>::from_slice(nonce_bytes);

            let plaintext = cipher.decrypt(&nonce, ciphertext)
                .map_err(|e| format!("Failed to decrypt gossip packet, ensure that you have provided the correct shared secret: {e:?}"))?;
            Ok(plaintext)
        }
    }

    impl<P: Eq + Hash + Serialize + DeserializeOwned, T: Versioned + Serialize + DeserializeOwned> GossipTransport
        for UdpGossipTransport<P, T>
    {
        type Id = P;
        type Address = SocketAddr;
        type State = T;

        async fn send(
            &self,
            address: Self::Address,
            msg: Message<Self::Id, Self::State>,
        ) -> Result<(), Box<dyn std::error::Error>> {
            let data = rmp_serde::to_vec(&msg)?;
            let encrypted_data = self.encrypt(&data)?;
            self.socket.send_to(&encrypted_data, address).await?;
            Ok(())
        }

        async fn try_receive(
            &self,
        ) -> Result<Option<(Self::Address, Message<Self::Id, Self::State>)>, Box<dyn std::error::Error>>
        {
            let mut buf = [0; 65507];
            match self.socket.try_recv_from(&mut buf) {
                Ok((size, addr)) => {
                    let decrypted_data = self.decrypt(&buf[..size])?;
                    let msg: Message<P, T> = rmp_serde::from_slice(&decrypted_data)?;
                    Ok(Some((addr, msg)))
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
                Err(e) => Err(Box::new(e)),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
    use rand::Rng;
    use std::net::SocketAddr;
    use tokio::time::{timeout, Duration, sleep};
    use crate::cluster::message::{Message, ClusterStateDigest};
    use crate::cluster::versioned::LastWriteWinsValue;
    use serde::{Serialize, Deserialize};

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
        async fn udp_gossip_transport_send_and_receive() {
            let shared_secret = [42u8; 32];
            let (addr1_str, addr1) = random_local_addr();
            let (addr2_str, addr2) = random_local_addr();

            let transport1 = UdpGossipTransport::<TestPeer, LastWriteWinsValue<i32>>::new(&addr1_str, shared_secret).await.unwrap();
            let transport2 = UdpGossipTransport::<TestPeer, LastWriteWinsValue<i32>>::new(&addr2_str, shared_secret).await.unwrap();

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

            let transport1 = UdpGossipTransport::<TestPeer, LastWriteWinsValue<i32>>::new(&addr1_str, shared_secret1).await.unwrap();
            let transport2 = UdpGossipTransport::<TestPeer, LastWriteWinsValue<i32>>::new(&addr2_str, shared_secret2).await.unwrap();

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
                        Ok(opt) => {
                            if opt.is_some() {
                                break Ok::<(), Box<dyn std::error::Error>>(());
                            } else {
                                sleep(Duration::from_millis(10)).await;
                            }
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
}
#[cfg(test)]
mod tests {
    use std::hash::Hash;

    use super::*;
    use tokio::sync::{Mutex, mpsc};

    pub struct InMemoryGossipTransport<P: Eq + Hash, T: Versioned> {
        sender: mpsc::Sender<Message<P, T>>,
        receiver: Mutex<mpsc::Receiver<Message<P, T>>>,
        peer_address: P,
        _phantom: std::marker::PhantomData<P>,
    }

    impl<P: Eq + Hash + Clone, T: Versioned> InMemoryGossipTransport<P, T> {
        pub fn new(addr1: P, addr2: P) -> (Self, Self) {
            let (tx1, rx1) = mpsc::channel(10);
            let (tx2, rx2) = mpsc::channel(10);

            (
                Self {
                    sender: tx1,
                    receiver: Mutex::new(rx2),
                    peer_address: addr2.clone(),
                    _phantom: std::marker::PhantomData,
                },
                Self {
                    sender: tx2,
                    receiver: Mutex::new(rx1),
                    peer_address: addr1,
                    _phantom: std::marker::PhantomData,
                },
            )
        }
    }

    impl<P, T> GossipTransport for InMemoryGossipTransport<P, T>
    where
        P: Eq + Hash + Clone + Send + 'static,
        T: Versioned + Send + 'static,
    {
        type Id = P;
        type Address = Self::Id;
        type State = T;

        async fn send(
            &self,
            _peer: Self::Address,
            msg: Message<P, T>,
        ) -> Result<(), Box<dyn std::error::Error>> {
            self.sender.send(msg).await?;
            Ok(())
        }

        async fn try_receive(
            &self,
        ) -> Result<Option<(Self::Address, Message<P, T>)>, Box<dyn std::error::Error>> {
            let msg = self
                .receiver
                .lock()
                .await
                .recv()
                .await
                .map(|msg| (self.peer_address.clone(), msg));
            Ok(msg)
        }
    }
}
