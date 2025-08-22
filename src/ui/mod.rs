use actix_web::{http::header::ContentType, web, App, HttpResponse, HttpServer, Result};
use include_dir::{include_dir, Dir};

use crate::history::HistoryProvider;

mod api;
mod page;

// Embed the dist directory at compile time
static ASSETS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/ui/dist");

#[derive(Clone)]
pub struct AppState<const N: usize> {
    pub config: crate::config::ConfigProvider,
    pub history: HistoryProvider<N>,
}

impl<const N: usize> AppState<N> {
    pub fn new(config: crate::config::ConfigProvider, history: HistoryProvider<N>) -> Self {
        Self { config, history }
    }
}

// Custom handler for serving embedded static files
async fn serve_static(path: web::Path<String>) -> Result<HttpResponse> {
    let file_path = path.into_inner();

    if let Some(file) = ASSETS_DIR.get_file(&file_path) {
        let mut response = HttpResponse::Ok();

        // Set appropriate content type
        match file_path.split('.').next_back().unwrap_or("") {
            "js" => response.insert_header(("content-type", "application/javascript")),
            "wasm" => response.insert_header(("content-type", "application/wasm")),
            "css" => response.insert_header(("content-type", "text/css")),
            "html" => response.insert_header(ContentType::html()),
            _ => response.insert_header(ContentType::octet_stream()),
        };

        Ok(response.body(file.contents()))
    } else {
        Ok(HttpResponse::NotFound().body("File not found"))
    }
}

pub fn create_app<const N: usize>() -> App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    App::new()
        .route("/", web::get().to(page::index::<N>))
        .route("/api/v1/probes", web::get().to(api::get_probes::<N>))
        .route(
            "/api/v1/probes/{probe}/history",
            web::get().to(api::get_history::<N>),
        )
        .route("/api/v1/notices", web::get().to(api::get_notices::<N>))
        .route("/static/{filename:.*}", web::get().to(serve_static))
}

pub async fn start_server<const N: usize>(
    config: crate::config::ConfigProvider,
    history: HistoryProvider<N>,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState::<N>::new(config.clone(), history);

    let listen_addr = config.ui().listen.clone();

    Ok(
        HttpServer::new(move || create_app::<N>().app_data(web::Data::new(state.clone())))
            .workers(1)
            .bind(&listen_addr)?
            .run()
            .await
            .map_err(|e| format!("{}", e))?,
    )
}
