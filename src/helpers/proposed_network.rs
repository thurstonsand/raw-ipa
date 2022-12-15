#![allow(dead_code, clippy::mutable_key_type)]

use crate::{
    helpers::{network::MessageChunks, ByteArrStream, Role},
    protocol::QueryId,
};
use futures::Stream;
use hyper::Uri;
use std::collections::HashMap;
use std::fmt::Debug;
use tokio::sync::oneshot;

#[derive(Debug, thiserror::Error)]
pub enum NetworkEventError {
    #[error("event {event_name} failed to respond to callback for query_id {}", .query_id.as_ref())]
    CallbackFailed {
        event_name: &'static str,
        query_id: QueryId,
    },
}

pub struct SendMessageData {
    pub chunks: MessageChunks,
}

impl SendMessageData {
    pub fn new(chunks: MessageChunks) -> Self {
        Self { chunks }
    }
}

/// Events sent within the context of executing a query
pub enum RingEvent {
    SendMessage(SendMessageData),
}

pub trait Ring: Sync {
    /// Type of the channel that is used to send messages to other helpers
    type Sink: futures::Sink<RingEvent, Error = NetworkEventError> + Send + Unpin + 'static;
    type MessageStream: Stream<Item = RingEvent> + Send + Unpin + 'static;

    /// Returns a sink that accepts data to be sent to other helper parties.
    fn sink(&self) -> Self::Sink;

    /// Returns a stream to receive messages that have arrived from other helpers. Note that
    /// some implementations may panic if this method is called more than once.
    fn recv_stream(&self) -> Self::MessageStream;
}

pub struct CreateQueryData {
    callback: oneshot::Sender<QueryId>,
}

impl CreateQueryData {
    #[must_use]
    pub fn new(callback: oneshot::Sender<QueryId>) -> Self {
        CreateQueryData { callback }
    }
    pub fn respond(self, query_id: QueryId) -> Result<(), NetworkEventError> {
        self.callback
            .send(query_id)
            .map_err(|_| NetworkEventError::CallbackFailed {
                event_name: "CreateQuery",
                query_id,
            })
    }
}

pub struct PrepareQueryData {
    pub query_id: QueryId,
    callback: oneshot::Sender<()>,
}

impl PrepareQueryData {
    #[must_use]
    pub fn new(query_id: QueryId, callback: oneshot::Sender<()>) -> Self {
        PrepareQueryData { query_id, callback }
    }

    pub fn respond(self) -> Result<(), NetworkEventError> {
        let query_id = self.query_id;
        self.callback
            .send(())
            .map_err(|_| NetworkEventError::CallbackFailed {
                event_name: "PrepareQuery",
                query_id,
            })
    }
}

pub struct StartMulData {
    pub query_id: QueryId,
    pub endpoints_positions: [Uri; 3],
    pub field_type: String,
    pub data_stream: ByteArrStream,
    callback: oneshot::Sender<()>,
}

impl StartMulData {
    pub fn new(
        query_id: QueryId,
        endpoints_positions: [Uri; 3],
        field_type: String,
        data_stream: ByteArrStream,
        callback: oneshot::Sender<()>,
    ) -> Self {
        StartMulData {
            query_id,
            endpoints_positions,
            field_type,
            data_stream,
            callback,
        }
    }

    pub fn respond(self) -> Result<(), NetworkEventError> {
        let query_id = self.query_id;
        self.callback
            .send(())
            .map_err(|_| NetworkEventError::CallbackFailed {
                event_name: "StartMul",
                query_id,
            })
    }
}

pub struct MulData {
    pub query_id: QueryId,
    pub endpoints_positions: [Uri; 3],
    pub endpoints_to_roles_and_stream: HashMap<Uri, (Role, ByteArrStream)>,
}

impl MulData {
    pub fn new(
        query_id: QueryId,
        endpoints_positions: [Uri; 3],
        endpoints_to_roles_and_stream: HashMap<Uri, (Role, ByteArrStream)>,
    ) -> Self {
        Self {
            query_id,
            endpoints_positions,
            endpoints_to_roles_and_stream,
        }
    }
}

pub struct RingEventData {
    pub query_id: QueryId,
    pub endpoints_to_roles: HashMap<Uri, Role>,
    pub ring_event: RingEvent,
}

impl RingEventData {
    pub fn new(
        query_id: QueryId,
        endpoints_to_roles: HashMap<Uri, Role>,
        ring_event: RingEvent,
    ) -> Self {
        Self {
            query_id,
            endpoints_to_roles,
            ring_event,
        }
    }
}

pub enum NetworkCommand {
    // Commands sent to handle query creation and initialization
    CreateQuery(CreateQueryData),
    PrepareQuery(PrepareQueryData),
    StartMul(StartMulData),
    Mul(MulData),

    // Commands sent within the context of a `Ring`, to be used internally
    RingEvent(RingEventData),
}

pub trait Network<R: Ring> {
    type EventStream: Stream<Item = NetworkCommand>;
    type Sink: futures::Sink<NetworkCommand, Error = NetworkEventError>;

    /// To be called by the entity which will handle events being emitted by `Network`.
    /// # Panics
    /// May panic if called more than once
    fn register(&self) -> Self::EventStream;

    /// To be called when an entity wants to send events to the `Network`.
    fn sink(&self) -> Self::Sink;

    /// Use when preparing to run a protocol. This [`Ring`] will enable messages to be sent/received
    /// within the context of a particular query, using relative [`Role`] positioning as defined
    /// for this query.
    fn assign_ring(
        query_id: QueryId,
        endpoints_positions: [Uri; 3],
        endpoints_to_roles: HashMap<Uri, Role>,
    ) -> R;
}
