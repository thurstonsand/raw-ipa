use crate::helpers::{
    query::QueryCommand,
    transport::{
        http::{
            server::{handlers::QueryConfigFromReq, Error},
            CreateQueryResp,
        },
        TransportCommand,
    },
    CommandEnvelope, CommandOrigin, HelperIdentity,
};
use axum::{routing::post, Extension, Json, Router};
use hyper::{Body, Request};
use tokio::sync::{mpsc, oneshot};

#[cfg_attr(feature = "enable-serde", derive(serde::Deserialize))]
struct CreateQueryBody {
    helper_positions: [HelperIdentity; 3],
}

/// Takes details from the HTTP request and creates a `[TransportCommand]::CreateQuery` that is sent
/// to the [`HttpTransport`]. HTTP request is deconstructed in order to leave parsing the `Body` for
/// last so that it can be rejected before parsing if needed.
async fn handler(
    transport_sender: Extension<mpsc::Sender<CommandEnvelope>>,
    query_config: QueryConfigFromReq,
    _req: Request<Body>,
) -> Result<Json<CreateQueryResp>, Error> {
    let permit = transport_sender.reserve().await?;

    // TODO: do we need this?
    // let Json(CreateQueryBody { helper_positions }) = RequestParts::new(req).extract().await?;

    // prepare command data
    let (tx, rx) = oneshot::channel();

    // send command, receive response
    let command = CommandEnvelope {
        origin: CommandOrigin::Other,
        payload: TransportCommand::Query(QueryCommand::Create(query_config.0, tx)),
    };
    permit.send(command);
    let query_id = rx.await?;

    Ok(Json(CreateQueryResp { query_id }))
}

pub fn router(transport_sender: mpsc::Sender<CommandEnvelope>) -> Router {
    Router::new()
        .route("/query", post(handler))
        .layer(Extension(transport_sender))
}
