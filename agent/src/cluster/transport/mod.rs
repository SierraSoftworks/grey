mod udp;
use std::hash::Hash;

use super::*;
pub use udp::UdpGossipTransport;

#[cfg(test)]
pub use tests::InMemoryGossipTransport;

pub trait GossipTransport<Id: Eq + Hash, State: Versioned> {
    type Address;

    fn send(
        &self,
        address: Self::Address,
        msg: Message<Id, State>,
    ) -> impl std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>;
    fn try_receive(
        &self,
    ) -> impl std::future::Future<
        Output = Result<
            Option<(Self::Address, Message<Id, State>)>,
            Box<dyn std::error::Error>,
        >,
    >;
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

    impl<P, T> GossipTransport<P, T> for InMemoryGossipTransport<P, T>
    where
        P: Eq + Hash + Clone + Send + 'static,
        T: Versioned + Send + 'static,
    {
        type Address = P;

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
