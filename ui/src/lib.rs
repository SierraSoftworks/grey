mod api;
mod auth;
mod client;
mod components;
mod contexts;
#[cfg(debug_assertions)]
pub mod demo;
pub mod formatters;
pub mod routes;
mod tones;
mod views;

// Export components for both SSR and WASM usage
pub use client::{App, AppProps};
pub use components::*;
pub use contexts::*;

/// The design system's assets, for the server to deliver alongside the page: the stylesheet, and
/// the font files it is set in.
#[cfg(feature = "ssr")]
pub mod assets {
    pub use yewi::{STYLESHEET, THEME_SCRIPT, fonts};
}
