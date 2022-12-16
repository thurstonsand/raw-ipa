#![allow(dead_code)]

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
    type RespData;
    fn respond(self, query_id: QueryId, data: Self::RespData) -> Result<(), NetworkCommandError>;
}

pub struct CreateQueryData {
    pub context_type: ContextType,
    pub field_type: String,
    pub helper_positions: [HelperIdentity; 3],
    callback: oneshot::Sender<(QueryId, <Self as NetworkCommandData>::RespData)>,
}

impl CreateQueryData {
    #[must_use]
    pub fn new(
        context_type: ContextType,
        field_type: String,
        helper_positions: [HelperIdentity; 3],
        callback: oneshot::Sender<(QueryId, <Self as NetworkCommandData>::RespData)>,
    ) -> Self {
        CreateQueryData {
            context_type,
            field_type,
            helper_positions,
            callback,
        }
    }
}

impl NetworkCommandData for CreateQueryData {
    type RespData = HelperIdentity;
    fn respond(self, query_id: QueryId, data: Self::RespData) -> Result<(), NetworkCommandError> {
        self.callback
            .send((query_id, data))
            .map_err(|_| NetworkCommandError::CallbackFailed {
                event_name: "CreateQuery",
                query_id,
            })
    }
}

pub struct PrepareQueryData {
    pub query_id: QueryId,
    pub context_type: ContextType,
    pub field_type: String,
    pub helper_positions: [HelperIdentity; 3],
    pub helpers_to_roles: HashMap<HelperIdentity, Role>,
    callback: oneshot::Sender<()>,
}

impl PrepareQueryData {
    #[must_use]
    pub fn new(
        query_id: QueryId,
        context_type: ContextType,
        field_type: String,
        helper_positions: [HelperIdentity; 3],
        helpers_to_roles: HashMap<HelperIdentity, Role>,
        callback: oneshot::Sender<()>,
    ) -> Self {
        PrepareQueryData {
            query_id,
            context_type,
            field_type,
            helper_positions,
            helpers_to_roles,
            callback,
        }
    }
}

impl NetworkCommandData for PrepareQueryData {
    type RespData = ();
    fn respond(self, query_id: QueryId, _: Self::RespData) -> Result<(), NetworkCommandError> {
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
    type RespData = ();
    fn respond(self, query_id: QueryId, _: Self::RespData) -> Result<(), NetworkCommandError> {
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
    pub field_type: String,
    pub destination: HelperIdentity,
    pub data: ByteArrStream,
}

impl MulData {
    pub fn new(
        query_id: QueryId,
        field_type: String,
        destination: HelperIdentity,
        data: ByteArrStream,
    ) -> Self {
        Self {
            query_id,
            field_type,
            destination,
            data,
        }
    }
}

impl NetworkCommandData for MulData {
    type RespData = ();
    fn respond(self, _: QueryId, _: Self::RespData) -> Result<(), NetworkCommandError> {
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
    type RespData = ();
    fn respond(self, _: QueryId, _: Self::RespData) -> Result<(), NetworkCommandError> {
        Ok(())
    }
}

pub enum NetworkCommand {
    // Commands sent to handle query creation and initialization

    // Helper which receives this command becomes the de facto leader of the query setup. It will:
    // * generate `query_id`
    // * assign roles to each helper, generating a role mapping `helper id` -> `role`
    // * store `query_id` -> (`context_type`, `field_type`, `secret share mapping`, `role mapping`)
    // * inform other helpers of new `query_id` and associated data
    // * respond with `query_id` and helper which should receive `Start*` command
    CreateQuery(CreateQueryData),

    // Helper which receives this message is a follower of the query setup. It will receive this
    // message from the leader, who has received the `CreateQuery` command. It will:
    // * store `query_id` -> (`context_type`, `field_type`, `secret share mapping`, `role mapping`)
    // * respond with ack
    PrepareQuery(PrepareQueryData),

    // Helper which receives this message is the leader of the mul protocol, as chosen by the leader
    // of the `CreateQuery` command. It will:
    // * retrieve (`context_type`, `field_type`, `secret share mapping`, `role mapping`)
    // * assign `Transport` using `secret share mapping` and `role mapping`
    // * break apart incoming data into 3 different streams, 1 for each helper
    // * send 2 of the streams to other helpers
    // * run the protocol using final stream of data, `context_type`, `field_type`
    StartMul(StartMulData),

    // Helper which receives this message is a follower of the mul protocol. It will:
    // * retrieve (`context_type`, `field_type`, `secret share mapping`, `role mapping`)
    // * assign `Transport` using `secret share mapping` and `role mapping`
    // * run the protocol using incoming stream of data, `context_type`, `field_type`
    Mul(MulData),

    // Commands sent within the context of a `Ring`, to be used internally
    TransportEvent(TransportEventData),
}

pub trait Network<T: Transport> {
    type CommandStream: Stream<Item = NetworkCommand>;
    type Sink: futures::Sink<NetworkCommand, Error = NetworkCommandError>;

    /// To be called by the entity which will handle events being emitted by `Network`. There should
    /// be only 1 subscriber
    /// # Panics
    /// May panic if called more than once
    fn subscribe(&self) -> Self::CommandStream;

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
