mod components;
mod client;

// Export components for both SSR and WASM usage
pub use components::*;
pub use client::{App, AppProps};

// Main entry point for trunk
#[allow(dead_code)]
fn main() {
    #[cfg(target_arch = "wasm32")]
    wasm_logger::init(wasm_logger::Config::default());
    #[cfg(feature = "wasm")]
    yew::Renderer::<App>::new().render();
}
