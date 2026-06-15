use yew_router::prelude::*;

/// The client-side routes served by the SPA. The status page lives at the root; the full incident
/// list and per-incident pages have their own routes. Unknown paths redirect home (see the `switch`
/// in `client.rs`).
#[derive(Clone, Routable, PartialEq)]
pub enum Route {
    #[at("/")]
    Home,
    #[at("/incidents")]
    Incidents,
    #[at("/incidents/:id")]
    Incident { id: String },
    #[not_found]
    #[at("/404")]
    NotFound,
}
