//! Serves the design system the UI is built from: yewi's stylesheet and the font files it is set
//! in. Both are compiled into the agent (through `grey-ui`) rather than emitted by Trunk, so they
//! are served from here instead of the embedded `ui/dist`.

use std::sync::LazyLock;

use actix_web::{HttpResponse, web};
use grey_ui::assets::{STYLESHEET, fonts};
use sha2::{Digest, Sha256};

use super::IMMUTABLE_CACHE_CONTROL;

/// A fingerprint of the assets, carried in their URLs. It changes whenever they do (a yewi
/// upgrade), which is what makes it safe to cache them forever under a given URL.
static FINGERPRINT: LazyLock<String> = LazyLock::new(|| {
    let mut hasher = Sha256::new();
    hasher.update(STYLESHEET.as_bytes());
    hasher.update(fonts::RELATIVE_CSS.as_bytes());
    hex::encode(&hasher.finalize()[..8])
});

/// The `<link>` elements for the document head. The fonts come first so the `@font-face` rules
/// are known by the time the stylesheet names the families.
pub fn head_links() -> String {
    let base = format!("/static/design/{}", *FINGERPRINT);
    format!(
        r#"<link rel="stylesheet" href="{base}/fonts.css"><link rel="stylesheet" href="{base}/yewi.css">"#
    )
}

/// `GET /static/design/{fingerprint}/{file}`: the stylesheet, the `@font-face` rules or one font
/// file. The fingerprint only keys the cache; whatever it says, the current assets are returned.
pub async fn serve(path: web::Path<(String, String)>) -> HttpResponse {
    let (_, file) = path.into_inner();

    let (content_type, body): (&str, &'static [u8]) = match file.as_str() {
        "yewi.css" => ("text/css", STYLESHEET.as_bytes()),
        // The rules name the font files relatively, so they resolve beside this file.
        "fonts.css" => ("text/css", fonts::RELATIVE_CSS.as_bytes()),
        name => match fonts::FILES.iter().find(|font| font.name == name) {
            Some(font) => (fonts::CONTENT_TYPE, font.bytes),
            None => return HttpResponse::NotFound().body("File not found"),
        },
    };

    HttpResponse::Ok()
        .insert_header(("content-type", content_type))
        .insert_header(("cache-control", IMMUTABLE_CACHE_CONTROL))
        .body(body)
}

#[cfg(test)]
mod tests {
    use actix_web::{App, test, web};

    use super::*;

    /// Everything the head links to, and every font the rules name, is served and cacheable.
    #[actix_web::test]
    async fn serves_the_stylesheet_and_fonts_as_immutable() {
        let app = test::init_service(
            App::new().route("/static/design/{fingerprint}/{file}", web::get().to(serve)),
        )
        .await;

        let links = head_links();
        let mut paths: Vec<String> = links
            .split("href=\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next().map(str::to_string))
            .collect();
        assert_eq!(paths.len(), 2, "expected the fonts and the stylesheet: {links}");
        let base = paths[0].rsplit_once('/').unwrap().0.to_string();
        paths.extend(fonts::FILES.iter().map(|font| format!("{base}/{}", font.name)));

        for path in paths {
            let resp = test::call_service(&app, test::TestRequest::get().uri(&path).to_request()).await;
            assert!(resp.status().is_success(), "{path} should be served");
            assert_eq!(
                resp.headers().get("cache-control").and_then(|v| v.to_str().ok()),
                Some(IMMUTABLE_CACHE_CONTROL),
                "{path} should be cached as immutable"
            );
            assert!(!test::read_body(resp).await.is_empty(), "{path} should have a body");
        }

        let missing = test::TestRequest::get().uri(&format!("{base}/nope.woff2")).to_request();
        assert_eq!(test::call_service(&app, missing).await.status(), 404);
    }
}
