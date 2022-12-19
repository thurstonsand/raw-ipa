#![allow(dead_code)]

use crate::{
    helpers::{
        network::MessageChunks,
        proposed_transport::{QueryEventData, Transport, TransportCommand},
        Error, HelperIdentity, Role,
    },
    protocol::QueryId,
    sync::Arc,
};
use futures::Stream;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::fmt::Debug;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug)]
pub struct SendMessageData {
    pub chunks: MessageChunks,
}

impl SendMessageData {
    pub fn new(chunks: MessageChunks) -> Self {
        Self { chunks }
    }
}

/// Events sent within the context of executing a query
#[derive(Debug)]
pub enum NetworkEvent {
    SendMessage(SendMessageData),
}

/// Given any implementation of [`Transport`], a `Network` is able to send and receive messages for
/// a specific query id. It will expose these query id-specific messages as [`NetworkEvent`]s.
pub struct Network<T> {
    transport: Arc<T>,
    query_id: QueryId,
    roles_to_helpers: HashMap<Role, HelperIdentity>,
}

impl<T: Transport> Network<T> {
    pub fn new(
        transport: Arc<T>,
        query_id: QueryId,
        roles_to_helpers: HashMap<Role, HelperIdentity>,
    ) -> Self {
        Self {
            transport,
            query_id,
            roles_to_helpers,
        }
    }

    /// sends a [`NetworkEvent`] on the underlying [`Transport`]
    pub async fn send(&self, network_event: NetworkEvent) -> Result<(), Error> {
        self.transport
            .send(TransportCommand::QueryEvent(QueryEventData {
                query_id: self.query_id,
                roles_to_helpers: self.roles_to_helpers.clone(),
                network_event,
            }))
            .await
            .map_err(Error::from)
    }

    /// returns a [`Stream`] of [`NetworkEvent`]s from the underlying [`Transport`]
    /// # Errors
    /// if called more than once during the execution of a query.
    pub fn recv_stream(&self) -> Result<impl Stream<Item = NetworkEvent>, Error> {
        let (tx, rx) = mpsc::channel(1);
        let query_id = self.query_id;
        let mut query_command_stream = self.transport.subscribe_to_query(query_id)?;

        tokio::spawn(async move {
            loop {
                match query_command_stream.next().await {
                    Some(TransportCommand::QueryEvent(QueryEventData {
                        network_event, ..
                    })) => match tx.send(network_event).await {
                        Ok(()) => continue,
                        Err(err) => {
                            tracing::error!("event for query id {} could not be delivered because recv_stream is closed: {err}", query_id.as_ref());
                            break;
                        }
                    },
                    Some(other_command) => tracing::error!(
                        "query id {} received unexpected command {other_command:?}",
                        query_id.as_ref()
                    ),
                    None => break,
                }
            }
        });
        Ok(ReceiverStream::new(rx))
    }
}
