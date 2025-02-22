//!
//! This module contains implementations and traits that enable protocols to communicate with
//! each other. In order for helpers to send messages, they need to know the destination. In some
//! cases this might be the exact address of helper host/instance (for example IP address), but
//! in many situations MPC helpers simply need to be able to send messages to the
//! corresponding helper without needing to know the exact location - this is what this module
//! enables MPC protocols to do.
//!
use crate::{
    helpers::buffers::ReceiveBuffer,
    helpers::error::Error,
    helpers::fabric::{ChannelId, MessageEnvelope, Network},
    helpers::Role,
    protocol::{RecordId, UniqueStepId},
};

use crate::ff::{Field, Int};
use crate::helpers::buffers::{SendBuffer, SendBufferConfig};
use futures::SinkExt;
use futures::StreamExt;
use std::fmt::{Debug, Formatter};
use std::io;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::Instrument;

/// Trait for messages sent between helpers
pub trait Message: Debug + Send + Sized + 'static {
    /// Required number of bytes to store this message on disk/network
    const SIZE_IN_BYTES: u32;

    /// Deserialize message from a sequence of bytes.
    ///
    /// ## Errors
    /// Returns an error if the provided buffer does not have enough bytes to read (EOF).
    fn deserialize(buf: &mut [u8]) -> io::Result<Self>;

    /// Serialize this message to a mutable slice. Implementations need to ensure `buf` has enough
    /// capacity to store this message.
    ///
    /// ## Errors
    /// Returns an error if `buf` does not have enough capacity to store at least `SIZE_IN_BYTES` more
    /// data.
    fn serialize(self, buf: &mut [u8]) -> io::Result<()>;
}

/// Any field value can be send as a message
impl<F: Field> Message for F {
    const SIZE_IN_BYTES: u32 = F::Integer::BITS / 8;

    fn deserialize(buf: &mut [u8]) -> io::Result<Self> {
        <F as Field>::deserialize(buf)
    }

    fn serialize(self, buf: &mut [u8]) -> io::Result<()> {
        <F as Field>::serialize(&self, buf)
    }
}

/// Entry point to the messaging layer managing communication channels for protocols and provides
/// the ability to send and receive messages from helper peers. Protocols request communication
/// channels to be open by calling `get_channel`, after that it is possible to send messages
/// through the channel end and request a given message type from helper peer.
///
/// Gateways are generic over `Network` meaning they can operate on top of in-memory communication
/// channels and real network.
///
/// ### Implementation details
/// Gateway, when created, runs an event loop in a dedicated tokio task that pulls the messages
/// from the networking layer and attempts to fulfil the outstanding requests to receive them.
/// If `receive` method on the channel has never been called, it puts the message to the local
/// buffer and keeps it there until such request is made by the protocol.
/// TODO: limit the size of the buffer and only pull messages when there is enough capacity
#[derive(Debug)]
pub struct Gateway {
    /// Sender end of the channel to send requests to receive messages from peers.
    tx: mpsc::Sender<ReceiveRequest>,
    envelope_tx: mpsc::Sender<(ChannelId, MessageEnvelope)>,
    control_handle: JoinHandle<()>,
}

/// Channel end
#[derive(Debug)]
pub struct Mesh<'a, 'b> {
    gateway: &'a Gateway,
    step: &'b UniqueStepId,
}

pub(super) struct ReceiveRequest {
    pub channel_id: ChannelId,
    pub record_id: RecordId,
    pub sender: oneshot::Sender<Box<[u8]>>,
}

impl Mesh<'_, '_> {
    /// Send a given message to the destination. This method will not return until the message
    /// is delivered to the `Network`.
    ///
    /// # Errors
    /// Returns an error if it fails to send the message or if there is a serialization error.
    pub async fn send<T: Message>(
        &self,
        dest: Role,
        record_id: RecordId,
        msg: T,
    ) -> Result<(), Error> {
        let mut buf = vec![0; T::SIZE_IN_BYTES as usize];
        msg.serialize(&mut buf)
            .map_err(|e| Error::serialization_error(record_id, self.step, e))?;

        let payload = buf.into_boxed_slice();
        let envelope = MessageEnvelope { record_id, payload };

        self.gateway
            .send(ChannelId::new(dest, self.step.clone()), envelope)
            .await
    }

    /// Receive a message that is associated with the given record id.
    ///
    /// # Errors
    /// Returns an error if it fails to receive the message or if a deserialization error occurred
    pub async fn receive<T: Message>(&self, source: Role, record_id: RecordId) -> Result<T, Error> {
        let mut payload = self
            .gateway
            .receive(ChannelId::new(source, self.step.clone()), record_id)
            .await?;

        let obj = T::deserialize(&mut payload)
            .map_err(|e| Error::serialization_error(record_id, self.step, e))?;

        Ok(obj)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GatewayConfig {
    /// Configuration for send buffers. See `SendBufferConfig` for more details
    pub send_buffer_config: SendBufferConfig,
    // ///
    // pub items_in_batch: u32,
    //
    // /// How many messages can be sent in parallel. This value is picked arbitrarily as
    // /// most unit tests don't send more than this value, so the setup does not have to
    // /// be annoying. `items_in_batch` * `batch_count` defines the total capacity for
    // /// send buffer. Increasing this value does not really impact the latency for tests
    // /// because they flush the data to network once they've accumulated at least
    // /// `items_in_batch` elements. Ofc setting it to some absurdly large value is going
    // /// to be problematic from memory perspective.
    // pub batch_count: u32,
}

impl Gateway {
    pub fn new<N: Network>(role: Role, network: &N, config: GatewayConfig) -> Self {
        let (tx, mut receive_rx) = mpsc::channel::<ReceiveRequest>(1);
        let (envelope_tx, mut envelope_rx) = mpsc::channel::<(ChannelId, MessageEnvelope)>(1);
        let mut message_stream = network.recv_stream();
        let mut network_sink = network.sink();

        let control_handle = tokio::spawn(async move {
            let mut receive_buf = ReceiveBuffer::default();
            let mut send_buf = SendBuffer::new(config.send_buffer_config);

            loop {
                // Make a random choice what to process next:
                // * Receive a message from another helper
                // * Handle the request to receive a message from another helper
                // * Send a message
                tokio::select! {
                    Some(receive_request) = receive_rx.recv() => {
                        tracing::trace!("new {:?}", receive_request);
                        receive_buf.receive_request(receive_request.channel_id, receive_request.record_id, receive_request.sender);
                    }
                    Some((channel_id, messages)) = message_stream.next() => {
                        tracing::trace!("received {} message(s) from {:?}", messages.len(), channel_id);
                        receive_buf.receive_messages(&channel_id, messages);
                    }
                    Some((channel_id, msg)) = envelope_rx.recv() => {
                        if let Some(buf_to_send) = send_buf.push(&channel_id, msg).expect("Failed to append data to the send buffer") {
                            tracing::trace!("sending {} message(s) to {:?}", buf_to_send.len(), &channel_id);
                            network_sink.send((channel_id, buf_to_send)).await
                                .expect("Failed to send data to the network");
                        }
                    }
                    else => {
                        tracing::debug!("All channels are closed and event loop is terminated");
                        break;
                    }
                }
            }
        }.instrument(tracing::info_span!("gateway_loop", role=?role)));

        Self {
            tx,
            envelope_tx,
            control_handle,
        }
    }

    /// Create or return an existing channel for a given step. Protocols can send messages to
    /// any helper through this channel (see `Mesh` interface for details).
    ///
    /// This method makes no guarantee that the communication channel will actually be established
    /// between this helper and every other one. The actual connection may be created only when
    /// `Mesh::send` or `Mesh::receive` methods are called.
    #[must_use]
    pub fn mesh<'a, 'b>(&'a self, step: &'b UniqueStepId) -> Mesh<'a, 'b> {
        Mesh {
            gateway: self,
            step,
        }
    }

    async fn receive(
        &self,
        channel_id: ChannelId,
        record_id: RecordId,
    ) -> Result<Box<[u8]>, Error> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(ReceiveRequest {
                channel_id: channel_id.clone(),
                record_id,
                sender: tx,
            })
            .await?;

        rx.await
            .map_err(|e| Error::receive_error(channel_id.role, e))
    }

    async fn send(&self, id: ChannelId, env: MessageEnvelope) -> Result<(), Error> {
        Ok(self.envelope_tx.send((id, env)).await?)
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        self.control_handle.abort();
    }
}

impl Debug for ReceiveRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ReceiveRequest({:?}, {:?})",
            self.channel_id, self.record_id
        )
    }
}
