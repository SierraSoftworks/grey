use askama::Template;
use tide::{http::mime, Request, Response};

use super::State;

#[derive(Template)]
#[template(path = "index.html")]
pub struct PageTemplate<'a> {
    pub config: &'a crate::config::UiConfig,
    pub availability: f64,
    pub probes: Vec<&'a crate::Probe>,
}

pub async fn index(req: Request<State>) -> tide::Result {
    let state = req.state();
    let probes = state
        .probes
        .values()
        .map(|probe| probe.as_ref())
        .collect::<Vec<_>>();

    let availability = if probes.is_empty() {
        100.0
    } else {
        probes.iter().map(|probe| probe.availability()).sum::<f64>() / (probes.len() as f64)
    };

    let template = PageTemplate {
        availability,
        config: &state.config,
        probes,
    };

    Ok(Response::builder(200)
        .content_type(mime::HTML)
        .body(template.render()?)
        .build())
}
