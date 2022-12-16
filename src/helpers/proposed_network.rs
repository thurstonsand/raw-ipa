#![allow(dead_code, clippy::mutable_key_type)]

use crate::{
    helpers::{network::MessageChunks, ByteArrStream, HelperIdentity, Role},
    protocol::{context::ContextType, QueryId},
};
use futures::Stream;
use std::collections::HashMap;
use std::fmt::Debug;
use tokio::sync::oneshot;

#[derive(Debug, thiserror::Error)]
pub enum NetworkCommandError {
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
pub enum TransportEvent {
    SendMessage(SendMessageData),
}

pub trait Transport: Sync {
    /// Type of the channel that is used to send messages to other helpers
    type Sink: futures::Sink<TransportEvent, Error = NetworkCommandError> + Send + Unpin + 'static;
    type MessageStream: Stream<Item = TransportEvent> + Send + Unpin + 'static;

    /// Returns a sink that accepts data to be sent to other helper parties.
    fn sink(&self) -> Self::Sink;

    /// Returns a stream to receive messages that have arrived from other helpers. Note that
    /// some implementations may panic if this method is called more than once.
    fn recv_stream(&self) -> Self::MessageStream;
}

pub trait NetworkCommandData {
    fn respond(self, query_id: QueryId) -> Result<(), NetworkCommandError>;
}

pub struct CreateQueryData {
    pub context_type: ContextType,
    pub helper_positions: [HelperIdentity; 3],
    pub field_type: String,
    callback: oneshot::Sender<QueryId>,
}

impl CreateQueryData {
    #[must_use]
    pub fn new(
        context_type: ContextType,
        helper_positions: [HelperIdentity; 3],
        field_type: String,
        callback: oneshot::Sender<QueryId>,
    ) -> Self {
        CreateQueryData {
            context_type,
            helper_positions,
            field_type,
            callback,
        }
    }
}

impl NetworkCommandData for CreateQueryData {
    fn respond(self, query_id: QueryId) -> Result<(), NetworkCommandError> {
        self.callback
            .send(query_id)
            .map_err(|_| NetworkCommandError::CallbackFailed {
                event_name: "CreateQuery",
                query_id,
            })
    }
}

pub struct PrepareQueryData {
    pub query_id: QueryId,
    pub context_type: ContextType,
    pub helper_positions: [HelperIdentity; 3],
    pub field_type: String,
    callback: oneshot::Sender<()>,
}

impl PrepareQueryData {
    #[must_use]
    pub fn new(
        query_id: QueryId,
        context_type: ContextType,
        helper_positions: [HelperIdentity; 3],
        field_type: String,
        callback: oneshot::Sender<()>,
    ) -> Self {
        PrepareQueryData {
            query_id,
            context_type,
            helper_positions,
            field_type,
            callback,
        }
    }
}

impl NetworkCommandData for PrepareQueryData {
    fn respond(self, query_id: QueryId) -> Result<(), NetworkCommandError> {
        self.callback
            .send(())
            .map_err(|_| NetworkCommandError::CallbackFailed {
                event_name: "PrepareQuery",
                query_id,
            })
    }
}

pub struct StartMulData {
    pub query_id: QueryId,
    pub data_stream: ByteArrStream,
    callback: oneshot::Sender<()>,
}

impl StartMulData {
    pub fn new(
        query_id: QueryId,
        data_stream: ByteArrStream,
        callback: oneshot::Sender<()>,
    ) -> Self {
        StartMulData {
            query_id,
            data_stream,
            callback,
        }
    }
}

impl NetworkCommandData for StartMulData {
    fn respond(self, query_id: QueryId) -> Result<(), NetworkCommandError> {
        self.callback
            .send(())
            .map_err(|_| NetworkCommandError::CallbackFailed {
                event_name: "StartMul",
                query_id,
            })
    }
}

pub struct MulData {
    pub query_id: QueryId,
    pub destination: HelperIdentity,
    pub data: ByteArrStream,
}

impl MulData {
    pub fn new(query_id: QueryId, destination: HelperIdentity, data: ByteArrStream) -> Self {
        Self {
            query_id,
            destination,
            data,
        }
    }
}

impl NetworkCommandData for MulData {
    fn respond(self, _: QueryId) -> Result<(), NetworkCommandError> {
        Ok(())
    }
}

pub struct TransportEventData {
    pub query_id: QueryId,
    pub roles_to_helpers: HashMap<Role, HelperIdentity>,
    pub transport_event: TransportEvent,
}

impl TransportEventData {
    pub fn new(
        query_id: QueryId,
        roles_to_helpers: HashMap<Role, HelperIdentity>,
        ring_event: TransportEvent,
    ) -> Self {
        Self {
            query_id,
            roles_to_helpers,
            transport_event: ring_event,
        }
    }
}

impl NetworkCommandData for TransportEventData {
    fn respond(self, _: QueryId) -> Result<(), NetworkCommandError> {
        Ok(())
    }
}

pub enum NetworkCommand {
    // Commands sent to handle query creation and initialization
    CreateQuery(CreateQueryData),
    PrepareQuery(PrepareQueryData),
    StartMul(StartMulData),
    Mul(MulData),

    // Commands sent within the context of a `Ring`, to be used internally
    TransportEvent(TransportEventData),
}

pub trait Network<T: Transport> {
    type CommandStream: Stream<Item = NetworkCommand>;
    type Sink: futures::Sink<NetworkCommand, Error = NetworkCommandError>;

    /// To be called by the entity which will handle events being emitted by `Network`.
    /// # Panics
    /// May panic if called more than once
    fn register(&self) -> Self::CommandStream;

    /// To be called when an entity wants to send events to the `Network`.
    fn sink(&self) -> Self::Sink;

    /// Use when preparing to run a protocol. This [`Transport`] will enable messages to be sent/received
    /// within the context of a particular query, using relative [`Role`] positioning as defined
    /// for this query.
    fn assign_transport(
        query_id: QueryId,
        helper_positions: [HelperIdentity; 3],
        helpers_to_roles: HashMap<HelperIdentity, Role>,
    ) -> T;
}
