#![allow(dead_code)]

use crate::{
    helpers::{
        network::MessageChunks,
        proposed_transport::{NetworkEventData, SubscriptionType, Transport, TransportCommand},
        Error, HelperIdentity, Role,
    },
    protocol::QueryId,
};
use futures::{Stream, StreamExt};
use std::collections::HashMap;

/// Given any implementation of [`Transport`], a `Network` is able to send and receive
/// [`MessageChunks`] for a specific query id. The [`Transport`] will receive `NetworkEvents`
/// containing the `MessageChunks`
pub struct Network<T> {
    transport: T,
    query_id: QueryId,
    roles_to_helpers: HashMap<Role, HelperIdentity>,
}

impl<T: Transport> Network<T> {
    pub fn new(
        transport: T,
        query_id: QueryId,
        roles_to_helpers: HashMap<Role, HelperIdentity>,
    ) -> Self {
        Self {
            transport,
            query_id,
            roles_to_helpers,
        }
    }

    /// sends a [`NetworkEvent`] containing [`MessageChunks`] on the underlying [`Transport`]
    pub async fn send(&self, message_chunks: MessageChunks) -> Result<(), Error> {
        let role = message_chunks.0.role;
        let destination = self.roles_to_helpers.get(&role).unwrap();
        self.transport
            .send(
                destination,
                TransportCommand::NetworkEvent(NetworkEventData {
                    query_id: self.query_id,
                    roles_to_helpers: self.roles_to_helpers.clone(),
                    message_chunks,
                }),
            )
            .await
            .map_err(Error::from)
    }

    /// returns a [`Stream`] of [`MessageChunks`]s from the underlying [`Transport`]
    /// # Panics
    /// if called more than once during the execution of a query.
    pub fn recv_stream(&self) -> impl Stream<Item = MessageChunks> {
        let query_id = self.query_id;
        let query_command_stream = self.transport.subscribe(SubscriptionType::Query(query_id));

        query_command_stream.map(move |command| match command {
            TransportCommand::NetworkEvent(NetworkEventData { message_chunks, .. }) => {
                message_chunks
            }
            other_command => panic!(
                "received unexpected command {other_command:?} for query id {}",
                query_id.as_ref()
            ),
        })
    }
}
