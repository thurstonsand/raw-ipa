use crate::helpers::{error::Error, Role};
use crate::protocol::{RecordId, UniqueStepId};
use async_trait::async_trait;
use futures::Stream;
use std::fmt::{Debug, Formatter};

/// Combination of helper role and step that uniquely identifies a single channel of communication
/// between two helpers.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct ChannelId {
    pub role: Role,
    pub step: UniqueStepId,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MessageEnvelope {
    pub record_id: RecordId,
    pub payload: Box<[u8]>,
}

pub type MessageChunks = (ChannelId, Vec<MessageEnvelope>);

/// Network interface for components that require communication.
#[async_trait]
pub trait Network: Sync {
    /// Type of the channel that is used to send messages to other helpers
    type Sink: futures::Sink<MessageChunks, Error = Error> + Send + Unpin + 'static;
    type MessageStream: Stream<Item = MessageChunks> + Send + Unpin + 'static;

    /// Returns a sink that accepts data to be sent to other helper parties.
    fn sink(&self) -> Self::Sink;

    /// Returns a stream to receive messages that have arrived from other helpers. Note that
    /// some implementations may panic if this method is called more than once.
    fn recv_stream(&self) -> Self::MessageStream;
}

impl ChannelId {
    #[must_use]
    pub fn new(role: Role, step: UniqueStepId) -> Self {
        Self { role, step }
    }
}

impl Debug for ChannelId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "channel[peer={:?},step={:?}]", self.role, self.step)
    }
}
