use actix_web::{App, HttpResponse, HttpServer, Result, http::header::ContentType, web};
use include_dir::{Dir, include_dir};

use crate::state::State;

mod api;
mod page;

// Embed the dist directory at compile time
static ASSETS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../ui/dist");

#[derive(Clone)]
pub struct AppState {
    pub state: State,
}

impl AppState {
    pub fn new(state: State) -> Self {
        Self { state }
    }

    #[cfg(test)]
    pub async fn test(temp_path: std::path::PathBuf) -> Self {
        Self { state: State::test(temp_path).await }
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

pub fn create_app() -> App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    App::new()
        .route("/", web::get().to(page::index))
        .route("/api/v1/probes", web::get().to(api::get_probes))
        .route("/api/v1/notices", web::get().to(api::get_notices))
        .route("/api/v1/cluster/peers", web::get().to(api::get_peers))
        .route("/static/{filename:.*}", web::get().to(serve_static))
}

pub async fn start_server(state: State) -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState::new(state);

    let listen_addr = state.state.get_config().ui.listen.clone();

    Ok(
        HttpServer::new(move || create_app().app_data(web::Data::new(state.clone())))
            .workers(1)
            .bind(&listen_addr)?
            .run()
            .await
            .map_err(|e| format!("{}", e))?,
    )
}
