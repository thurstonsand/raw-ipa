use crate::{
    helpers::{error::Error, MessagePayload, Role},
    protocol::{RecordId, Step},
};
use async_trait::async_trait;
use futures::{ready, Stream};
use pin_project::pin_project;
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;
use tokio_util::sync::{PollSendError, PollSender};

/// Combination of helper role and step that uniquely identifies a single channel of communication
/// between two helpers.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct ChannelId {
    pub role: Role,
    pub step: Step,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MessageEnvelope {
    pub record_id: RecordId,
    pub payload: MessagePayload,
}

pub type MessageChunks = (ChannelId, Vec<u8>);

/// Network interface for components that require communication.
#[async_trait]
pub trait Network: Sync {
    /// Type of the channel that is used to send/receive messages to/from other helpers
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
    pub fn new(role: Role, step: Step) -> Self {
        Self { role, step }
    }
}

impl Debug for ChannelId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "channel[{:?},{:?}]", self.role, self.step)
    }
}

/// Wrapper around a [`PollSender`] to modify the error message to match what the [`NetworkSink`]
/// requires. The only error that [`PollSender`] will generate is "channel closed", and thus is the
/// only error message forwarded from this [`NetworkSink`].
#[pin_project]
pub struct NetworkSink<T, E> {
    #[pin]
    inner: PollSender<T>,
    _phantom: PhantomData<E>,
}

impl<T: Send + 'static, E> NetworkSink<T, E> {
    #[must_use]
    pub fn new(sender: mpsc::Sender<T>) -> Self {
        Self {
            inner: PollSender::new(sender),
            _phantom: PhantomData::default(),
        }
    }
}

impl<T: Send + 'static, E: From<PollSendError<T>>> futures::Sink<T> for NetworkSink<T, E> {
    type Error = E;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.project().inner.poll_ready(cx)?);
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        self.project().inner.start_send(item)?;
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.project().inner.poll_flush(cx)?);
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.project().inner.poll_close(cx))?;
        Poll::Ready(Ok(()))
    }
}
