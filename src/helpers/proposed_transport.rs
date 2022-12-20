#![allow(dead_code)]

use crate::{
    helpers::{network::MessageChunks, ByteArrStream, HelperIdentity, Role},
    protocol::{context::ContextType, QueryId},
};
use async_trait::async_trait;
use futures::Stream;
use std::collections::HashMap;
use std::fmt::Debug;
use tokio::sync::oneshot;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("command {command_name} failed to respond to callback for query_id {}", .query_id.as_ref())]
    CallbackFailed {
        command_name: &'static str,
        query_id: QueryId,
    },
    /// TODO: this error may return "missing" data due to nature of `SendPollError`. This should be
    ///       improved
    #[error("command {} failed to send data for query {}",
    .command_name.unwrap_or("<missing_command>"),
    if let Some(q) =.query_id { q.as_ref() } else { "<missing query id>" }
    )]
    SendFailed {
        command_name: Option<&'static str>,
        query_id: Option<QueryId>,
    },
    #[error("attempted to subscribe to commands for query id {}, but there is already a previous subscriber", .query_id.as_ref())]
    PreviouslySubscribed { query_id: QueryId },
}

impl From<tokio_util::sync::PollSendError<TransportCommand>> for TransportError {
    fn from(source: tokio_util::sync::PollSendError<TransportCommand>) -> Self {
        let (command_name, query_id) = match source.into_inner() {
            Some(TransportCommand::CreateQuery(_)) => (Some(CreateQueryData::name()), None),
            Some(TransportCommand::PrepareQuery(PrepareQueryData { query_id, .. })) => {
                (Some(PrepareQueryData::name()), Some(query_id))
            }
            Some(TransportCommand::StartMul(StartMulData { query_id, .. })) => {
                (Some(StartMulData::name()), Some(query_id))
            }
            Some(TransportCommand::Mul(MulData { query_id, .. })) => {
                (Some(MulData::name()), Some(query_id))
            }
            Some(TransportCommand::NetworkEvent(NetworkEventData { query_id, .. })) => {
                (Some(NetworkEventData::name()), Some(query_id))
            }
            None => (None, None),
        };
        Self::SendFailed {
            command_name,
            query_id,
        }
    }
}

pub trait TransportCommandData {
    type RespData;
    fn name() -> &'static str;
    fn respond(self, query_id: QueryId, data: Self::RespData) -> Result<(), TransportError>;
}

#[derive(Debug)]
pub struct CreateQueryData {
    pub context_type: ContextType,
    pub field_type: String,
    pub helper_positions: [HelperIdentity; 3],
    callback: oneshot::Sender<(QueryId, <Self as TransportCommandData>::RespData)>,
}

impl CreateQueryData {
    #[must_use]
    pub fn new(
        context_type: ContextType,
        field_type: String,
        helper_positions: [HelperIdentity; 3],
        callback: oneshot::Sender<(QueryId, <Self as TransportCommandData>::RespData)>,
    ) -> Self {
        CreateQueryData {
            context_type,
            field_type,
            helper_positions,
            callback,
        }
    }
}

impl TransportCommandData for CreateQueryData {
    type RespData = HelperIdentity;
    fn name() -> &'static str {
        "CreateQuery"
    }

    fn respond(self, query_id: QueryId, data: Self::RespData) -> Result<(), TransportError> {
        self.callback
            .send((query_id, data))
            .map_err(|_| TransportError::CallbackFailed {
                command_name: Self::name(),
                query_id,
            })
    }
}

#[derive(Debug)]
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

impl TransportCommandData for PrepareQueryData {
    type RespData = ();
    fn name() -> &'static str {
        "PrepareQuery"
    }
    fn respond(self, query_id: QueryId, _: Self::RespData) -> Result<(), TransportError> {
        self.callback
            .send(())
            .map_err(|_| TransportError::CallbackFailed {
                command_name: Self::name(),
                query_id,
            })
    }
}

#[derive(Debug)]
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

impl TransportCommandData for StartMulData {
    type RespData = ();
    fn name() -> &'static str {
        "StartMul"
    }
    fn respond(self, query_id: QueryId, _: Self::RespData) -> Result<(), TransportError> {
        self.callback
            .send(())
            .map_err(|_| TransportError::CallbackFailed {
                command_name: Self::name(),
                query_id,
            })
    }
}

#[derive(Debug)]
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

impl TransportCommandData for MulData {
    type RespData = ();
    fn name() -> &'static str {
        "Mul"
    }
    fn respond(self, _: QueryId, _: Self::RespData) -> Result<(), TransportError> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct NetworkEventData {
    pub query_id: QueryId,
    pub roles_to_helpers: HashMap<Role, HelperIdentity>,
    pub message_chunks: MessageChunks,
}

impl NetworkEventData {
    pub fn new(
        query_id: QueryId,
        roles_to_helpers: HashMap<Role, HelperIdentity>,
        message_chunks: MessageChunks,
    ) -> Self {
        Self {
            query_id,
            roles_to_helpers,
            message_chunks,
        }
    }
}

impl TransportCommandData for NetworkEventData {
    type RespData = ();
    fn name() -> &'static str {
        "NetworkEvent"
    }
    fn respond(self, _: QueryId, _: Self::RespData) -> Result<(), TransportError> {
        Ok(())
    }
}

#[derive(Debug)]
pub enum TransportCommand {
    // `Administration` Commands

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

    // `Query` Commands

    // message via `subscribe_to_query` method
    NetworkEvent(NetworkEventData),
}

/// Users of a [`Transport`] must subscribe to a specific type of command, and so must pass this
/// type as argument to the `subscribe` function
pub enum SubscriptionType {
    /// Commands for managing queries
    Administration,
    /// Commands intended for a running query
    Query(QueryId),
}

#[async_trait]
pub trait Transport: Sync {
    type CommandStream: Stream<Item = TransportCommand> + Send + Unpin + 'static;
    type Sink: futures::Sink<TransportCommand, Error = TransportError> + Send + Unpin + 'static;

    /// To be called by an entity which will handle the events as indicated by the
    /// [`SubscriptionType`]. There should be only 1 subscriber per type.
    /// # Panics
    /// May panic if attempt to subscribe to the same [`SubscriptionType`] twice
    fn subscribe(&self, subscription_type: SubscriptionType) -> Self::CommandStream;

    /// To be called when an entity wants to send commands to the `Transport`.
    async fn send(
        &self,
        destination: &HelperIdentity,
        command: TransportCommand,
    ) -> Result<(), TransportError>;
}
