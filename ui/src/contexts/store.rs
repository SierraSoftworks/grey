//! The SPA's single, integrated state store.
//!
//! [`Store`] is a redux/vuex-style container for every piece of shared UI state — the public config,
//! the signed-in administrator, the cluster peers, notices, incidents and probes — together with the
//! mutation operations the UI needs (sign in/out and the incident create/edit/delete flows). It also
//! owns the background polling that keeps the live entities fresh and the OIDC session bootstrap.
//!
//! State lives in a [`Reducible`] [`StoreState`] driven through a `use_reducer` handle, so every
//! change flows through a single [`Action`]. Components read state and dispatch actions via
//! [`use_store`]; they never construct requests or manage timers themselves.

use std::rc::Rc;

use grey_api::{
    AdminUser, ApiError, CreateIncident, Identifier, Incident, IncidentEdit, Peer, Probe, UiConfig,
    UiNotice,
};
use yew::prelude::*;

use crate::api::ApiClient;

/// Sorts incidents most-recently-updated first (those with no updates sort last), mirroring the
/// server's ordering so optimistic insertions land in the right place.
fn sort_incidents(incidents: &mut [Incident]) {
    incidents.sort_by(|a, b| b.last_updated().cmp(&a.last_updated()));
}

/// Inserts or replaces an incident in the shared (public) list. The list mirrors the unauthenticated
/// view, so an incident that is now hidden is dropped rather than shown.
fn apply_incident_upsert(incidents: &mut Vec<Incident>, incident: Incident) {
    incidents.retain(|i| i.id != incident.id);
    if incident.is_public() {
        incidents.push(incident);
    }
    sort_incidents(incidents);
}

/// The complete, observable UI state. Cloned on every mutation; comparisons are cheap enough for the
/// `use_reducer_eq` equality check that suppresses no-op re-renders.
#[derive(Clone, Default, PartialEq)]
pub struct StoreState {
    pub config: UiConfig,
    /// The signed-in administrator, or `None` for an anonymous visitor.
    pub user: Option<AdminUser>,
    /// The current ID token, mirrored here so views can gate admin UI and pass it to children.
    pub token: Option<String>,
    pub peers: Vec<Peer>,
    pub notices: Vec<UiNotice>,
    pub incidents: Vec<Incident>,
    pub probes: Vec<Probe>,
    /// The most recent background-fetch failure, surfaced to the user as a dismissible banner.
    pub error: Option<ApiError>,
}

/// Every state transition the store understands. All mutations — from background polls to admin
/// edits — are expressed as one of these and applied by [`StoreState::reduce`].
pub enum Action {
    SetProbes(Vec<Probe>),
    SetNotices(Vec<UiNotice>),
    SetPeers(Vec<Peer>),
    SetIncidents(Vec<Incident>),
    /// Insert or replace a single incident (after an admin create or edit), without waiting for the
    /// next poll.
    UpsertIncident(Incident),
    /// Remove a single incident (after an admin delete).
    RemoveIncident(Identifier),
    /// Record an established session (the validated administrator and their ID token).
    SetSession {
        user: AdminUser,
        token: Option<String>,
    },
    /// Drop the session (sign-out, or a token the agent no longer accepts).
    ClearSession,
    /// Record a background-fetch failure so the UI can surface it.
    SetError(ApiError),
    /// Dismiss the current error (the user closed the banner).
    ClearError,
}

impl Reducible for StoreState {
    type Action = Action;

    fn reduce(self: Rc<Self>, action: Self::Action) -> Rc<Self> {
        let mut next = (*self).clone();
        match action {
            Action::SetProbes(probes) => next.probes = probes,
            Action::SetNotices(notices) => next.notices = notices,
            Action::SetPeers(peers) => next.peers = peers,
            Action::SetIncidents(mut incidents) => {
                sort_incidents(&mut incidents);
                next.incidents = incidents;
            }
            Action::UpsertIncident(incident) => {
                apply_incident_upsert(&mut next.incidents, incident)
            }
            Action::RemoveIncident(id) => next.incidents.retain(|incident| incident.id != id),
            Action::SetSession { user, token } => {
                next.user = Some(user);
                next.token = token;
            }
            Action::ClearSession => {
                next.user = None;
                next.token = None;
            }
            Action::SetError(error) => next.error = Some(error),
            Action::ClearError => next.error = None,
        }
        Rc::new(next)
    }
}

/// The shared store handle exposed to the component tree. Cheap to clone (it holds an `Rc`-backed
/// reducer handle, the API client and the two session callbacks). Selectors read the current state;
/// the `*_incident` methods perform the matching API call and fold the result back into the store.
#[derive(Clone, PartialEq)]
pub struct Store {
    state: UseReducerHandle<StoreState>,
    client: ApiClient,
    /// Begins an interactive (popup) sign-in.
    pub login: Callback<()>,
    /// Clears the current session and returns the UI to an anonymous state.
    pub logout: Callback<()>,
    /// Dismisses the current error banner.
    pub clear_error: Callback<()>,
}

impl Store {
    // --- Selectors ------------------------------------------------------------------------------

    pub fn config(&self) -> &UiConfig {
        &self.state.config
    }

    pub fn probes(&self) -> &[Probe] {
        &self.state.probes
    }

    pub fn notices(&self) -> &[UiNotice] {
        &self.state.notices
    }

    pub fn peers(&self) -> &[Peer] {
        &self.state.peers
    }

    pub fn incidents(&self) -> &[Incident] {
        &self.state.incidents
    }

    pub fn user(&self) -> Option<&AdminUser> {
        self.state.user.as_ref()
    }

    /// The current ID token, if a session is established.
    pub fn token(&self) -> Option<String> {
        self.state.token.clone()
    }

    pub fn is_authenticated(&self) -> bool {
        self.state.user.is_some()
    }

    /// Whether OIDC admin auth is configured at all (so the UI knows to offer a sign-in control).
    pub fn auth_configured(&self) -> bool {
        self.state.config.auth.is_some()
    }

    /// The most recent unacknowledged background-fetch failure, if any.
    pub fn error(&self) -> Option<&ApiError> {
        self.state.error.as_ref()
    }

    /// The shared API client, for the admin-only reads (hidden drafts) that are fetched on demand
    /// rather than polled into the store.
    pub fn client(&self) -> &ApiClient {
        &self.client
    }

    // --- Mutations ------------------------------------------------------------------------------

    /// Creates an incident and reflects it in the shared list immediately.
    pub async fn create_incident(&self, input: CreateIncident) -> Result<Incident, ApiError> {
        let created = self.client.create_incident(&input).await?;
        self.state.dispatch(Action::UpsertIncident(created.clone()));
        Ok(created)
    }

    /// Saves an incident (check-and-set on `version`) and folds the authoritative result back in.
    pub async fn save_incident(
        &self,
        id: String,
        version: u64,
        edit: IncidentEdit,
    ) -> Result<Incident, ApiError> {
        let saved = self.client.replace_incident(&id, version, &edit).await?;
        self.state.dispatch(Action::UpsertIncident(saved.clone()));
        Ok(saved)
    }

    /// Deletes an incident and drops it from the shared list.
    pub async fn delete_incident(&self, id: Identifier) -> Result<(), ApiError> {
        self.client.delete_incident(&id.to_string()).await?;
        self.state.dispatch(Action::RemoveIncident(id));
        Ok(())
    }

    /// Surfaces an error in the shared banner. Used by the on-demand admin reads (hidden drafts)
    /// that bypass the store's own mutations, so their failures reach the same banner as everything
    /// else instead of being shown inline.
    pub fn set_error(&self, error: ApiError) {
        self.state.dispatch(Action::SetError(error));
    }
}

#[derive(Properties, PartialEq)]
pub struct StoreProviderProps {
    #[prop_or_default]
    pub config: UiConfig,
    #[prop_or_default]
    pub notices: Vec<UiNotice>,
    #[prop_or_default]
    pub probes: Vec<Probe>,
    #[prop_or_default]
    pub peers: Vec<Peer>,
    #[prop_or_default]
    pub incidents: Vec<Incident>,
    pub children: Children,
}

#[function_component(StoreProvider)]
pub fn store_provider(props: &StoreProviderProps) -> Html {
    let auth_cfg = props.config.auth.clone();
    let client = ApiClient::new(auth_cfg.clone());

    let state = use_reducer_eq({
        let config = props.config.clone();
        let notices = props.notices.clone();
        let probes = props.probes.clone();
        let peers = props.peers.clone();
        let incidents = props.incidents.clone();
        move || {
            let mut incidents = incidents;
            sort_incidents(&mut incidents);
            StoreState {
                config,
                user: None,
                token: None,
                peers,
                notices,
                incidents,
                probes,
                error: None,
            }
        }
    });

    // On mount, finish any pending OIDC callback and then validate the stored token by fetching the
    // current user. Effects only run client-side, so this never executes during SSR.
    {
        let state = state.clone();
        let auth_cfg = auth_cfg.clone();
        let client = client.clone();
        use_effect_with((), move |_| {
            #[cfg(feature = "wasm")]
            if let Some(cfg) = auth_cfg {
                wasm_bindgen_futures::spawn_local(async move {
                    let mut current = crate::auth::stored_token();
                    if crate::auth::has_pending_callback() {
                        match crate::auth::complete_callback(&cfg).await {
                            Ok(Some(t)) => current = Some(t),
                            Ok(None) => {}
                            Err(err) => gloo::console::error!(format!("OIDC sign-in failed: {err}")),
                        }
                    }
                    if current.is_some() {
                        // `me` transparently renews an expired token before failing, so a successful
                        // result means the session is valid.
                        match client.me().await {
                            Ok(user) => state.dispatch(Action::SetSession {
                                user,
                                token: crate::auth::stored_token(),
                            }),
                            // The stored token is no longer accepted; drop it.
                            Err(_) => crate::auth::clear_token(),
                        }
                    }
                });
            }
            #[cfg(not(feature = "wasm"))]
            let _ = (&state, &auth_cfg, &client);
            || ()
        });
    }

    // Background polling for the public, live entities. Polling pauses while the page is unfocused
    // and resumes — with an immediate catch-up fetch — when focus returns, so a backgrounded tab
    // doesn't keep hitting the agent (see [`focus::FocusTracker`]). Gated on the wasm32 target (not
    // just the feature) because it touches browser globals at render time via `use_focus_tracker`,
    // which would panic if compiled into a native test binary.
    #[cfg(all(feature = "wasm", target_arch = "wasm32"))]
    {
        let reload = props.config.reload_interval;
        let focus = use_focus_tracker();

        // Probes, notices and incidents: fetch immediately when the page was rendered without their
        // data (a minimal/un-hydrated render), otherwise the first fetch lands after one interval.
        {
            let state = state.clone();
            let client = client.clone();
            let focus = focus.clone();
            let seeded = !state.probes.is_empty();
            use_effect_with((), move |_| {
                wasm_bindgen_futures::spawn_local(async move {
                    if !seeded {
                        state.dispatch(load_probes(&client).await);
                        state.dispatch(load_notices(&client).await);
                        state.dispatch(load_incidents(&client).await);
                    }
                    loop {
                        gloo::timers::future::sleep(reload).await;
                        // Hold here while the page is unfocused; the interval that elapsed in the
                        // background collapses into a single fetch once focus returns.
                        focus.active().await;
                        state.dispatch(load_probes(&client).await);
                        state.dispatch(load_notices(&client).await);
                        state.dispatch(load_incidents(&client).await);
                    }
                });
                || ()
            });
        }

        // Peers (cluster topology) are operator-only and change often, so refresh them on mount and
        // then on the same interval.
        {
            let state = state.clone();
            let client = client.clone();
            let focus = focus.clone();
            use_effect_with((), move |_| {
                wasm_bindgen_futures::spawn_local(async move {
                    state.dispatch(Action::SetPeers(load_peers(&client).await));
                    loop {
                        gloo::timers::future::sleep(reload).await;
                        focus.active().await;
                        state.dispatch(Action::SetPeers(load_peers(&client).await));
                    }
                });
                || ()
            });
        }
    }

    let login = {
        let auth_cfg = auth_cfg.clone();
        let client = client.clone();
        let state = state.clone();
        Callback::from(move |_| {
            #[cfg(feature = "wasm")]
            if let Some(cfg) = auth_cfg.clone() {
                let client = client.clone();
                let state = state.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    match crate::auth::begin_login(&cfg).await {
                        Ok(Some(_)) => match client.me().await {
                            Ok(user) => state.dispatch(Action::SetSession {
                                user,
                                token: crate::auth::stored_token(),
                            }),
                            Err(err) => {
                                gloo::console::error!(format!("Sign-in validation failed: {err}"));
                                crate::auth::clear_token();
                            }
                        },
                        Ok(None) => {}
                        Err(err) => gloo::console::error!(format!("Sign-in failed: {err}")),
                    }
                });
            }
            #[cfg(not(feature = "wasm"))]
            let _ = (&auth_cfg, &client, &state);
        })
    };

    let logout = {
        let state = state.clone();
        Callback::from(move |_| {
            crate::auth::clear_token();
            state.dispatch(Action::ClearSession);
        })
    };

    let clear_error = {
        let state = state.clone();
        Callback::from(move |_| state.dispatch(Action::ClearError))
    };

    let store = Store {
        state,
        client,
        login,
        logout,
        clear_error,
    };

    html! {
        <ContextProvider<Store> context={store}>
            { props.children.clone() }
        </ContextProvider<Store>>
    }
}

#[hook]
pub fn use_store() -> Store {
    use_context::<Store>()
        .expect("Store not found. Make sure to wrap your component with StoreProvider.")
}

/// Provides a process-wide [`focus::FocusTracker`], created once and shared by every polling loop so
/// they register a single pair of focus/blur listeners.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
#[hook]
fn use_focus_tracker() -> focus::FocusTracker {
    (*use_memo((), |_| focus::FocusTracker::new())).clone()
}

// Background fetch helpers translate an API call into the action that folds its result into the
// store, logging (and flagging) failures so a transient error doesn't wedge the polling loop.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
async fn load_probes(client: &ApiClient) -> Action {
    match client.probes().await {
        Ok(probes) => Action::SetProbes(probes),
        Err(err) => {
            gloo::console::error!(format!("Failed to fetch probes: {err}"));
            Action::SetError(err)
        }
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
async fn load_notices(client: &ApiClient) -> Action {
    match client.notices().await {
        Ok(notices) => Action::SetNotices(notices),
        Err(err) => {
            gloo::console::error!(format!("Failed to fetch notices: {err}"));
            Action::SetError(err)
        }
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
async fn load_incidents(client: &ApiClient) -> Action {
    match client.incidents().await {
        Ok(incidents) => Action::SetIncidents(incidents),
        Err(err) => {
            gloo::console::error!(format!("Failed to fetch incidents: {err}"));
            Action::SetError(err)
        }
    }
}

/// Cluster topology is operator-only, so it is fetched only when signed in; a revoked/expired
/// session (or any error) is treated as "no peers" rather than spamming the error channel.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
async fn load_peers(client: &ApiClient) -> Vec<Peer> {
    if crate::auth::stored_token().is_none() {
        return Vec::new();
    }
    client.peers().await.unwrap_or_default()
}

/// Page-focus tracking used to pause background polling while the user is away.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod focus {
    use std::cell::RefCell;
    use std::future::Future;
    use std::pin::Pin;
    use std::rc::Rc;
    use std::task::{Context, Poll, Waker};

    use gloo::events::EventListener;
    use web_sys::window;

    struct Inner {
        active: bool,
        /// Futures parked in [`FocusTracker::active`] while the page is unfocused, woken on the next
        /// return to focus.
        wakers: Vec<Waker>,
        /// Kept alive so the focus/blur listeners stay registered for the tracker's lifetime.
        _listeners: Vec<EventListener>,
    }

    /// Tracks whether the page currently has focus. Background polling awaits [`active`](Self::active)
    /// after each interval, so a fetch that comes due while the page is unfocused is held until focus
    /// returns and then runs once. Cheap to clone (shares one `Rc`).
    #[derive(Clone)]
    pub struct FocusTracker {
        inner: Rc<RefCell<Inner>>,
    }

    impl FocusTracker {
        pub fn new() -> Self {
            // Seed from the document's current focus; assume focused if it can't be determined.
            let active = window()
                .and_then(|w| w.document())
                .and_then(|d| d.has_focus().ok())
                .unwrap_or(true);

            let inner = Rc::new(RefCell::new(Inner {
                active,
                wakers: Vec::new(),
                _listeners: Vec::new(),
            }));

            if let Some(window) = window() {
                let on_focus = {
                    let inner = inner.clone();
                    EventListener::new(&window, "focus", move |_| set_active(&inner, true))
                };
                let on_blur = {
                    let inner = inner.clone();
                    EventListener::new(&window, "blur", move |_| set_active(&inner, false))
                };
                inner.borrow_mut()._listeners = vec![on_focus, on_blur];
            }

            Self { inner }
        }

        /// Resolves as soon as the page is focused — immediately when it already is, otherwise on the
        /// next return to focus.
        pub fn active(&self) -> impl Future<Output = ()> {
            ActiveFuture {
                inner: self.inner.clone(),
            }
        }
    }

    /// Updates the focus flag, waking anything parked in [`FocusTracker::active`] on a transition
    /// back to focused.
    fn set_active(inner: &Rc<RefCell<Inner>>, active: bool) {
        let mut inner = inner.borrow_mut();
        let was_active = inner.active;
        inner.active = active;
        if active && !was_active {
            for waker in inner.wakers.drain(..) {
                waker.wake();
            }
        }
    }

    struct ActiveFuture {
        inner: Rc<RefCell<Inner>>,
    }

    impl Future for ActiveFuture {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            let mut inner = self.inner.borrow_mut();
            if inner.active {
                Poll::Ready(())
            } else {
                inner.wakers.push(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}
