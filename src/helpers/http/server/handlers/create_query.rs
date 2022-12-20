use crate::{
    ff::FieldTypeStr,
    helpers::{
        http::server::Error,
        proposed_transport::{CreateQueryData, TransportCommand},
        HelperIdentity,
    },
    protocol::{context::ContextType, QueryId},
};
use async_trait::async_trait;
use axum::{
    extract::{FromRequest, Query, RequestParts},
    routing::post,
    Extension, Json, Router,
};
use hyper::{Body, Request};
use tokio::sync::{mpsc, oneshot};

#[cfg_attr(feature = "enable-serde", derive(serde::Deserialize))]
struct CreateQueryParams {
    context_type: ContextType,
    field_type: String,
}

#[async_trait]
impl<B: Send> FromRequest<B> for CreateQueryParams {
    type Rejection = Error;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let Query(cqf) = req.extract::<Query<CreateQueryParams>>().await?;
        let _ = cqf.field_type.size_in_bytes()?; // confirm that `field_type` is valid
        Ok(cqf)
    }
}

#[cfg_attr(feature = "enable-serde", derive(serde::Deserialize))]
struct CreateQueryBody {
    helper_positions: [HelperIdentity; 3],
}

#[cfg_attr(feature = "enable-serde", derive(serde::Serialize))]
struct Resp {
    query_id: QueryId,
    target: HelperIdentity,
}

/// Takes details from the HTTP request and creates a `[TransportCommand]::CreateQuery` that is sent
/// to the [`HttpTransport`]. HTTP request is deconstructed in order to leave parsing the `Body` for
/// last so that it can be rejected before parsing if needed.
async fn handler(
    transport_sender: Extension<mpsc::Sender<TransportCommand>>,
    params: CreateQueryParams,
    req: Request<Body>,
) -> Result<Json<Resp>, Error> {
    let permit = transport_sender.0.reserve().await?;

    let Json(CreateQueryBody { helper_positions }) = RequestParts::new(req).extract().await?;

    // prepare command data
    let (tx, rx) = oneshot::channel();
    let data = CreateQueryData::new(params.context_type, params.field_type, helper_positions, tx);

    // send command, receive response
    permit.send(TransportCommand::CreateQuery(data));
    let (query_id, target) = rx.await?;

    Ok(Json(Resp { query_id, target }))
}

pub fn router(transport_sender: mpsc::Sender<TransportCommand>) -> Router {
    Router::new()
        .route("/query", post(handler))
        .layer(Extension(transport_sender))
}
