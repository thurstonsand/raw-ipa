use crate::helpers::http::server::Error;
use crate::helpers::proposed_transport::{PrepareQueryData, TransportCommand};
use crate::helpers::{HelperIdentity, Role};
use crate::protocol::context::ContextType;
use crate::protocol::QueryId;
use async_trait::async_trait;
use axum::extract::{FromRequest, Path, Query, RequestParts};
use axum::http::Request;
use axum::routing::post;
use axum::{Extension, Json, Router};
use hyper::Body;
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};

#[cfg_attr(feature = "enable-serde", derive(serde::Deserialize))]
struct PrepareQueryParams {
    context_type: ContextType,
    field_type: String,
}

#[cfg_attr(feature = "enable-serde", derive(serde::Deserialize))]
struct PrepareQueryBody {
    helper_positions: [HelperIdentity; 3],
    helpers_to_roles: HashMap<HelperIdentity, Role>,
}

#[async_trait]
impl<B: Send> FromRequest<B> for CreateQueryParams {
    type Rejection = Error;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let Query(pqf) = req.extract::<Query<CreateQueryParams>>().await?;
        let _ = pqf.field_type.size_in_bytes()?; // confirm that `field_type` is valid
        Ok(pqf)
    }
}

async fn handler(
    query_id: Path<QueryId>,
    params: PrepareQueryParams,
    transport_sender: Extension<mpsc::Sender<TransportCommand>>,
    req: Request<Body>,
) -> Result<(), Error> {
    let permit = transport_sender.0.reserve().await?;

    let Json(PrepareQueryBody {
        helper_positions,
        helpers_to_roles,
    }) = RequestParts::new(req).extract().await?;

    // prepare command data
    let (tx, rx) = oneshot::channel();
    let data = PrepareQueryData::new(
        query_id.0,
        params.context_type,
        params.field_type,
        helper_positions,
        helpers_to_roles,
        tx,
    );
    permit.send(TransportCommand::PrepareQuery(data));

    rx.await?;
    Ok(())
}

pub fn router(transport_sender: mpsc::Sender<TransportCommand>) -> Router {
    Router::new()
        .route("query/:query_id", post(handler))
        .layer(Extension(transport_sender))
}
