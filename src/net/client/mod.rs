mod error;

pub use error::MpcClientError;

use crate::{
    helpers::Identity,
    net::RecordHeaders,
    protocol::{QueryId, UniqueStepId},
};
use axum::{
    body::Bytes,
    http::{
        uri::{self, PathAndQuery},
        Request,
    },
};
use hyper::{client::HttpConnector, Body, Client, Uri};
use hyper_tls::HttpsConnector;

pub struct HttpSendMessagesArgs<'a> {
    pub query_id: &'a QueryId,
    pub step: &'a UniqueStepId,
    pub identity: Identity,
    pub offset: u32,
    pub data_size: u32,
    pub messages: Bytes,
}

#[allow(clippy::module_name_repetitions)] // follows standard naming convention
#[derive(Clone)]
pub struct MpcClient {
    client: Client<HttpsConnector<HttpConnector>>,
    scheme: uri::Scheme,
    authority: uri::Authority,
}

impl MpcClient {
    /// addr must have a valid scheme and authority
    /// # Panics
    /// if addr does not have scheme and authority
    #[must_use]
    pub fn new(addr: Uri) -> Self {
        // this works for both http and https
        let https = HttpsConnector::new();
        let client = Client::builder().build::<_, Body>(https);
        let parts = addr.into_parts();
        Self {
            client,
            scheme: parts.scheme.unwrap(),
            authority: parts.authority.unwrap(),
        }
    }

    /// same as new, but first parses the addr from a [&str]
    /// # Errors
    /// if addr is an invalid [Uri], this will fail
    pub fn with_str_addr(addr: &str) -> Result<Self, MpcClientError> {
        Ok(Self::new(addr.parse()?))
    }

    pub fn origin(&self) -> String {
        format!("{}://{}", self.scheme.as_str(), self.authority.as_str())
    }

    fn build_uri<T>(&self, p_and_q: T) -> Result<Uri, MpcClientError>
    where
        PathAndQuery: TryFrom<T>,
        <PathAndQuery as TryFrom<T>>::Error: Into<axum::http::Error>,
    {
        Ok(uri::Builder::new()
            .scheme(self.scheme.clone())
            .authority(self.authority.clone())
            .path_and_query(p_and_q)
            .build()?)
    }

    /// Responds with whatever input is passed to it
    /// # Errors
    /// If the request has illegal arguments, or fails to deliver to helper
    pub async fn echo(&self, s: &str) -> Result<Vec<u8>, MpcClientError> {
        let uri = self.build_uri(format!("/echo?foo={}", s))?;

        let response = self.client.get(uri).await?;
        let result = hyper::body::to_bytes(response.into_body()).await?;
        Ok(result.to_vec())
    }

    /// Sends a batch of messages to another helper. Messages are a contiguous block of records in
    /// some state of transformation within a protocol. Also includes ['`RecordId`] information and
    /// [`ChannelId`].
    /// # Errors
    /// If the request has illegal arguments, or fails to deliver to helper
    pub async fn send_messages(
        &self,
        args: HttpSendMessagesArgs<'_>,
    ) -> Result<(), MpcClientError> {
        let uri = self.build_uri(format!(
            "/mul/query-id/{}/step/{}?identity={}",
            args.query_id.as_ref(),
            args.step.as_ref(),
            args.identity.as_ref(),
        ))?;
        #[allow(clippy::cast_possible_truncation)] // `messages.len` is known to be smaller than u32
        let headers = RecordHeaders {
            content_length: args.messages.len() as u32,
            offset: args.offset,
            data_size: args.data_size,
        };
        let req = headers
            .add_to(Request::post(uri))
            .body(Body::from(args.messages))?;
        let response = self.client.request(req).await?;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            Err(MpcClientError::from_failed_resp(response).await)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::fabric::{ChannelId, MessageChunks, MessageEnvelope};
    use crate::net::{BindTarget, MpcServer};
    use hyper_tls::native_tls::TlsConnector;
    use tokio::sync::mpsc;

    async fn mul_req(client: MpcClient, mut rx: mpsc::Receiver<MessageChunks>) {
        const DATA_SIZE: u32 = 4;
        const DATA_LEN: u32 = 3;
        let query_id = QueryId;
        let step = UniqueStepId::default().narrow("mul_test");
        let identity = Identity::H1;
        let offset = 0;
        let messages = &[0; (DATA_SIZE * DATA_LEN) as usize];

        client
            .send_messages(HttpSendMessagesArgs {
                query_id: &query_id,
                step: &step,
                identity,
                offset,
                data_size: DATA_SIZE,
                messages: Bytes::from_static(messages),
            })
            .await
            .expect("send should succeed");

        let channel_id = ChannelId { identity, step };
        let env = [0; DATA_SIZE as usize].to_vec().into_boxed_slice();
        let envs = (0..DATA_LEN)
            .map(|i| MessageEnvelope {
                record_id: i.into(),
                payload: env.clone(),
            })
            .collect::<Vec<_>>();
        let server_recvd = rx.try_recv().unwrap(); // should already have been received
        assert_eq!(server_recvd, (channel_id, envs));
    }

    #[tokio::test]
    async fn mul_req_http() {
        let (tx, rx) = mpsc::channel(1);
        let server = MpcServer::new(tx);
        // setup server
        let (addr, _) = server
            .bind(BindTarget::Http("127.0.0.1:0".parse().unwrap()))
            .await;

        // setup client
        let client =
            MpcClient::with_str_addr(&format!("http://localhost:{}", addr.port())).unwrap();

        // test
        mul_req(client, rx).await;
    }

    #[tokio::test]
    async fn mul_req_https() {
        // setup server
        let (tx, rx) = mpsc::channel(1);
        let server = MpcServer::new(tx);
        let config = crate::net::server::tls_config_from_self_signed_cert()
            .await
            .unwrap();
        let (addr, _) = server
            .bind(BindTarget::Https("127.0.0.1:0".parse().unwrap(), config))
            .await;

        // setup client
        // requires custom client to use self signed certs
        let conn = TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();
        let mut http = HttpConnector::new();
        http.enforce_http(false);
        let https = HttpsConnector::<HttpConnector>::from((http, conn.into()));
        let hyper_client = hyper::Client::builder().build(https);
        let client = MpcClient {
            client: hyper_client,
            scheme: uri::Scheme::HTTPS,
            authority: uri::Authority::try_from(format!("localhost:{}", addr.port())).unwrap(),
        };

        // test
        mul_req(client, rx).await;
    }
}
