use yew_router::prelude::*;

/// The client-side routes served by the SPA. The status page lives at the root; the full incident
/// list and per-incident pages have their own routes. The `/auth/*` routes handle the OIDC login
/// popup callback and an explicit sign-out link. Unknown paths redirect home (see the `switch` in
/// `client.rs`).
#[derive(Clone, Routable, PartialEq)]
pub enum Route {
    #[at("/")]
    Home,
    #[at("/incidents")]
    Incidents,
    #[at("/incidents/new")]
    NewIncident,
    #[at("/incidents/:id")]
    Incident { id: String },
    /// Where the OIDC provider redirects after login; the SPA finishes the code exchange here.
    #[at("/auth/callback")]
    AuthCallback,
    /// Signs the current user out, then redirects home. Exposed as a route so it can be linked to.
    #[at("/auth/logout")]
    AuthLogout,
    #[not_found]
    #[at("/404")]
    NotFound,
}
