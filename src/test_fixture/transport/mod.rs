pub mod network;
mod routing;

use crate::helpers::{
    CommandEnvelope, HelperIdentity, SubscriptionType, Transport, TransportCommand, TransportError,
};
use crate::sync::Arc;
use async_trait::async_trait;
use routing::Switch;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use tokio::sync::mpsc::{channel, Sender};
use tokio_stream::wrappers::ReceiverStream;

pub struct InMemoryTransport {
    identity: HelperIdentity,
    peer_connections: HashMap<HelperIdentity, Sender<TransportCommand>>,
    switch: Switch,
}

impl Debug for InMemoryTransport {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "transport[{:?}]", self.identity)
    }
}

impl InMemoryTransport {
    pub fn new(identity: HelperIdentity) -> Self {
        Self {
            identity: identity.clone(),
            peer_connections: HashMap::default(),
            switch: Switch::new(identity),
        }
    }

    /// Establish a unidirectional connection to the given peer
    pub fn connect(&mut self, dest: &mut Self) {
        let (tx, rx) = channel(1);
        self.peer_connections.insert(dest.identity.clone(), tx);
        dest.switch.new_peer(self.identity.clone(), rx);
    }

    pub fn identity(&self) -> &HelperIdentity {
        &self.identity
    }

    pub fn listen(&mut self) {
        self.switch.listen();
    }

    #[cfg(all(test, feature = "shuttle"))]
    pub fn halt(&self) {
        /// this hackery needs to be explained. In normal circumstances (when you use tokio
        /// scheduler) explicit switch termination is not required because tokio drops all tasks
        /// during runtime shutdown. Other schedulers (ahem shuttle) may not do that and what
        /// happens is 3 switch tasks remain blocked awaiting messages from each other. In this
        /// case a deadlock is detected. Hence this code just tries to explicitly close the switch
        /// but because async drop is not a thing yet, we must hot loop here to drive it to completion
        let mut f = self.switch.halt();
        ::tokio::pin!(f);
        while f.poll_unpin(&mut Context::from_waker(futures::task::noop_waker_ref()))
            != Poll::Ready(())
        {
            std::thread::yield_now()
        }
    }
}

#[async_trait]
impl Transport for Arc<InMemoryTransport> {
    type CommandStream = ReceiverStream<CommandEnvelope>;

    async fn subscribe(&self, subscription_type: SubscriptionType) -> Self::CommandStream {
        match subscription_type {
            SubscriptionType::Administration => {
                unimplemented!()
            }
            SubscriptionType::Query(query_id) => self.switch.query_stream(query_id).await,
        }
    }

    async fn send(
        &self,
        destination: &HelperIdentity,
        command: TransportCommand,
    ) -> Result<(), TransportError> {
        Ok(self
            .peer_connections
            .get(destination)
            .unwrap()
            .send(command)
            .await?)
    }
}
