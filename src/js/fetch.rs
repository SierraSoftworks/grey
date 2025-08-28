use boa_engine::{
    Context, JsData, JsResult
};
use boa_gc::{Finalize, Trace};
use boa_runtime::fetch::{request::JsRequest, response::JsResponse, Fetcher};
use tracing_batteries::prelude::*;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::version;

#[derive(Clone, Debug, Trace, Finalize, JsData, Default)]
pub(crate) struct ReqwestFetcher {
    #[unsafe_ignore_trace]
    client: reqwest::Client,
}

impl Fetcher for ReqwestFetcher {
    async fn fetch(self: Rc<Self>, request: JsRequest, _context: &RefCell<&mut Context>) -> JsResult<JsResponse> {
        let client = self.client.clone();

        use boa_engine::{JsError, JsString};

        let request = request.into_inner();
        let url = request.uri().to_string();
        let req = client
            .request(request.method().clone(), &url)
            .header("User-Agent", version!("SierraSoftworks/grey@v"));

        // Inject trace headers automatically
        let mut trace_headers = HashMap::new();
        tracing_batteries::prelude::opentelemetry::global::get_text_map_propagator(|p| {
            p.inject_context(&Span::current().context(), &mut trace_headers)
        });
        let req = trace_headers.into_iter().fold(req, |req, (k, v)| {
            req.header(k.as_str(), v)
        });

        let req = req
            .headers(request.headers().clone())
            .body(request.body().clone())
            .build()
            .map_err(JsError::from_rust)?;

        let resp = client.execute(req).await.map_err(JsError::from_rust)?;

        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.bytes().await.map_err(JsError::from_rust)?;
        let mut builder = http::Response::builder().status(status.as_u16());

        for k in headers.keys() {
            for v in headers.get_all(k) {
                builder = builder.header(k.as_str(), v);
            }
        }

        builder
            .body(bytes.to_vec())
            .map_err(JsError::from_rust)
            .map(|response| JsResponse::basic(JsString::from(url), response))
    }
}