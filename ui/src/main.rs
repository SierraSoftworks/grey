mod api;
mod auth;
mod client;
mod components;
mod contexts;
#[cfg(debug_assertions)]
pub mod demo;
pub mod formatters;
pub mod routes;
mod styles;
mod views;

// Export components for both SSR and WASM usage
pub use client::{App, AppProps};
pub use components::*;
pub use contexts::*;

// Main entry point for trunk
#[allow(dead_code)]
fn main() {
    #[cfg(target_arch = "wasm32")]
    wasm_logger::init(wasm_logger::Config::default());

    // `?demo` renders the app from a local fixture with the API disabled, so the layout can be
    // iterated on with nothing but `trunk serve`.
    #[cfg(all(feature = "wasm", debug_assertions))]
    if demo::enabled() {
        yew::Renderer::<demo::DemoApp>::new().render();
        return;
    }

    #[cfg(feature = "wasm")]
    if let Ok(props) = AppProps::from_dom() {
        yew::Renderer::<App>::with_props(props).hydrate();
    } else if let Ok(props) = AppProps::from_dom_minimal() {
        yew::Renderer::<App>::with_props(props).render();
    } else {
        yew::Renderer::<App>::with_props(AppProps::default()).render();
    }
}
