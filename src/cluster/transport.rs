use std::{hash::Hash, net::SocketAddr, str::FromStr, sync::Arc};

use super::*;

use serde::{Serialize, de::DeserializeOwned};
#[cfg(test)]
pub use tests::InMemoryGossipTransport;
use tokio::net::UdpSocket;

pub trait GossipTransport {
    type Peer: Eq + Hash;
    type Address;
    type State;

    fn send(
        &self,
        address: Self::Address,
        msg: Message<Self::Peer, Self::State>,
    ) -> impl std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>;
    fn try_receive(
        &self,
    ) -> impl std::future::Future<
        Output = Result<
            Option<(Self::Address, Message<Self::Peer, Self::State>)>,
            Box<dyn std::error::Error>,
        >,
    >;
}

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

impl<P: Eq + Hash + Serialize + DeserializeOwned, T: Serialize + DeserializeOwned> GossipTransport
    for UdpGossipTransport<P, T>
{
    type Peer = P;
    type Address = SocketAddr;
    type State = T;

    async fn send(
        &self,
        address: Self::Address,
        msg: Message<Self::Peer, Self::State>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let data = rmp_serde::to_vec(&msg)?;
        let encrypted_data = self.encrypt(&data)?;
        self.socket.send_to(&encrypted_data, address).await?;
        Ok(())
    }

    async fn try_receive(
        &self,
    ) -> Result<Option<(Self::Address, Message<Self::Peer, Self::State>)>, Box<dyn std::error::Error>>
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
    use std::hash::Hash;

    use super::*;
    use tokio::sync::{Mutex, mpsc};

    pub struct InMemoryGossipTransport<P: Eq + Hash, T> {
        sender: mpsc::Sender<Message<P, T>>,
        receiver: Mutex<mpsc::Receiver<Message<P, T>>>,
        peer_address: P,
        _phantom: std::marker::PhantomData<P>,
    }

    impl<P: Eq + Hash + Clone, T> InMemoryGossipTransport<P, T> {
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
        T: Clone + Send + 'static,
    {
        type Peer = P;
        type Address = Self::Peer;
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
