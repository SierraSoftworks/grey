use std::{collections::HashMap, sync::Arc};

use actix_web::{web, App, HttpServer, HttpResponse, Result, http::header::ContentType};
use include_dir::{include_dir, Dir};

use crate::Probe;

mod api;
mod page;

// Embed the dist directory at compile time
static ASSETS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/dist");

#[derive(Clone)]
pub struct AppState {
    pub ui_config: crate::config::UiConfig,
    pub probes: HashMap<String, Arc<Probe>>,
}

// Custom handler for serving embedded static files
async fn serve_static(path: web::Path<String>) -> Result<HttpResponse> {
    let file_path = path.into_inner();
    
    if let Some(file) = ASSETS_DIR.get_file(&file_path) {
        let mut response = HttpResponse::Ok();
        
        // Set appropriate content type
        match file_path.split('.').last().unwrap_or("") {
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

pub fn create_app() -> App<impl actix_web::dev::ServiceFactory<actix_web::dev::ServiceRequest, Config = (), Response = actix_web::dev::ServiceResponse, Error = actix_web::Error, InitError = ()>> {
    App::new()
        .route("/", web::get().to(page::index))
        .route("/api/v1/probes", web::get().to(api::get_probes))
        .route("/api/v1/probes/{probe}/history", web::get().to(api::get_history))
        .route("/api/v1/user-interface", web::get().to(api::get_ui_config))
        .route("/static/{filename:.*}", web::get().to(serve_static))
}

pub async fn start_server(config: crate::config::UiConfig, probes: Vec<Arc<Probe>>) -> std::io::Result<()> {
    let mut state = AppState {
        ui_config: config.clone(),
        probes: HashMap::new(),
    };

    for probe in probes {
        state.probes.insert(probe.name.clone(), probe);
    }

    let listen_addr = config.listen.clone();
    
    HttpServer::new(move || {
        create_app().app_data(web::Data::new(state.clone()))
    })
    .bind(&listen_addr)?
    .run()
    .await
}
