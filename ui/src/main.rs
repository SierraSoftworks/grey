mod client;
mod components;
mod contexts;

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
    yew::Renderer::<App>::with_props(AppProps::from_dom().unwrap()).hydrate();
}
