use yew_router::prelude::*;

/// The client-side routes served by the SPA. The status page lives at the root; the full incident
/// list has its own page. Unknown paths redirect home (see the `switch` in `client.rs`).
#[derive(Clone, Routable, PartialEq)]
pub enum Route {
    #[at("/")]
    Home,
    #[at("/incidents")]
    Incidents,
    #[not_found]
    #[at("/404")]
    NotFound,
}
