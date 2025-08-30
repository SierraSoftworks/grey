mod client;
mod components;
mod contexts;
pub mod formatters;

// Export components for both SSR and WASM usage
pub use client::{App, AppProps};
pub use components::*;
pub use contexts::*;

// Main entry point for trunk
#[allow(dead_code)]
fn main() {
    #[cfg(target_arch = "wasm32")]
    wasm_logger::init(wasm_logger::Config::default());

    #[cfg(feature = "wasm")]
    if let Ok(props) = AppProps::from_dom() {
        yew::Renderer::<App>::with_props(props).hydrate();
    } else if let Ok(props) = AppProps::from_dom_minimal() {
        yew::Renderer::<App>::with_props(props).render();
    } else {
        yew::Renderer::<App>::with_props(AppProps::default()).render();
    }
}
