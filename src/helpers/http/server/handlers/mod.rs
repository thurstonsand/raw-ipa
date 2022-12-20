use crate::helpers::proposed_transport::TransportCommand;
use axum::Router;
use tokio::sync::mpsc;

mod create_query;
mod echo;
mod prepare_query;

/// After an [`mpsc::OwnedPermit`] has been reserved, it can be used once to send an item on the channel.
///
/// Panics if cloned while a permit exists. the `Clone` implementation must exist so that
/// `ReservedPermit` can be added to a request via an `Extension`, which requires `Clone`. However,
/// Axum/Tower do not clone the request between middleware and the handler, so this is a safe usage.  
pub struct ReservedPermit<T>(Option<mpsc::OwnedPermit<T>>);

impl<T: Send + 'static> ReservedPermit<T> {
    pub fn new(permit: mpsc::OwnedPermit<T>) -> Self {
        Self(Some(permit))
    }
    /// # Panics
    /// if called more than once
    pub fn send(&mut self, item: T) {
        self.0
            .take()
            .expect("should only call `send` once")
            .send(item);
    }
}

impl<T> Clone for ReservedPermit<T> {
    /// # Panics
    /// if a permit exists
    fn clone(&self) -> Self {
        assert!(self.0.is_none());
        Self(None)
    }
}

pub fn router(transport_sender: mpsc::Sender<TransportCommand>) -> Router {
    echo::router()
        .merge(create_query::router(transport_sender))
        .merge(prepare_query::router())
}
