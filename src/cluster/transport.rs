use std::{hash::Hash, net::SocketAddr, str::FromStr, sync::Arc};

use super::*;

use serde::{de::DeserializeOwned, Serialize};
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
    _phantom: std::marker::PhantomData<(P, T)>,
}

impl<P, T> UdpGossipTransport<P, T> {
    pub async fn new(addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let addr = SocketAddr::from_str(addr)?;
        let socket = UdpSocket::bind(addr).await?;

        Ok(Self {
            socket: Arc::new(socket),
            _phantom: std::marker::PhantomData,
        })
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
        self.socket.send_to(&data, address).await?;
        Ok(())
    }

    async fn try_receive(
        &self,
    ) -> Result<Option<(Self::Address, Message<Self::Peer, Self::State>)>, Box<dyn std::error::Error>>
    {
        let mut buf = [0; 65507];
        match self.socket.try_recv_from(&mut buf) {
            Ok((size, addr)) => {
                let msg: Message<P, T> = rmp_serde::from_slice(&buf[..size])?;
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
    use tokio::sync::{mpsc, Mutex};

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
