mod components;
mod client;

// Export components for both SSR and WASM usage
pub use components::*;
pub use client::{ClientApp, ServerApp, ServerAppProps};

// WASM-specific functionality
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

// Main entry point for trunk
#[allow(dead_code)]
#[cfg(feature = "wasm")]
fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<ClientApp>::new().render();
}

// Also provide the hydration function for manual use
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn hydrate_app() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<ClientApp>::new().hydrate();
}
