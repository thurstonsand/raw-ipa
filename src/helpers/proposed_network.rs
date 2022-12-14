use crate::{
    helpers::{network::Network as Ring, ByteArrStream, Role},
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

pub struct CreateQueryData {
    callback: oneshot::Sender<QueryId>,
}

impl CreateQueryData {
    #[must_use]
    pub fn new(callback: oneshot::Sender<QueryId>) -> Self {
        CreateQueryData { callback }
    }
    pub fn respond(mut self, query_id: QueryId) -> Result<(), NetworkEventError> {
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

    pub fn respond(mut self) -> Result<(), NetworkEventError> {
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
    pub field_type: String,
    pub data_stream: ByteArrStream,
    callback: oneshot::Sender<()>,
}

impl StartMulData {
    pub fn new(
        query_id: QueryId,
        field_type: String,
        data_stream: ByteArrStream,
        callback: oneshot::Sender<()>,
    ) -> Self {
        StartMulData {
            query_id,
            field_type,
            data_stream,
            callback,
        }
    }

    pub fn respond(mut self) -> Result<(), NetworkEventError> {
        let query_id = self.query_id;
        self.callback
            .send(())
            .map_err(|_| NetworkEventError::CallbackFailed {
                event_name: "StartMul",
                query_id,
            })
    }
}

pub struct AssignRingData<N: Network> {
    pub query_id: QueryId,
    pub endpoints_positions: [Uri; 3],
    pub endpoints_to_roles: HashMap<Uri, Role>,
    callback: oneshot::Sender<N::Ring>,
}

impl<N: Network> AssignRingData<N> {
    pub fn new(
        query_id: QueryId,
        endpoints_positions: [Uri; 3],
        endpoints_to_roles: HashMap<Uri, Role>,
        callback: oneshot::Sender<N::Ring>,
    ) -> Self {
        Self {
            query_id,
            endpoints_positions,
            endpoints_to_roles,
            callback,
        }
    }

    pub fn respond(mut self, ring: N::Ring) -> Result<(), NetworkEventError> {
        let query_id = self.query_id;
        self.callback
            .send(ring)
            .map_err(|_| NetworkEventError::CallbackFailed {
                event_name: "AssignRing",
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

pub enum NetworkEvent<N: Network> {
    CreateQuery(CreateQueryData),
    PrepareQuery(PrepareQueryData),
    StartMul(StartMulData),
    AssignRing(AssignRingData<N>),
    Mul(MulData),
}

pub trait Network: Sized {
    type Ring: Ring;
    // TODO: this seems odd
    type EventStream: Stream<Item = NetworkEvent<Self>>;
    type Sink: futures::Sink<NetworkEvent<Self>, Error = NetworkEventError>;

    /// To be called by the entity which will handle events being emitted by `Network`.
    /// # Panics
    /// May panic if called more than once
    fn register(&self) -> Self::EventStream;

    /// To be called when an entity wants to send events to the `Network`.
    fn sink(&self) -> Self::Sink;
}
