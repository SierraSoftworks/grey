use yew::prelude::*;
use yew_router::prelude::*;

use crate::contexts::use_store;
use crate::routes::Route;

/// Signs the current user out and returns to the status page. Exposed as a dedicated route so a
/// plain link can trigger sign-out from anywhere (the header's user chip points here too).
#[function_component(AuthLogout)]
pub fn auth_logout() -> Html {
    let store = use_store();
    let navigator = use_navigator();

    {
        let logout = store.logout.clone();
        let navigator = navigator.clone();
        // Clear the stored session on mount, then return home. Effects only run client-side, so SSR
        // simply renders the message below.
        use_effect_with((), move |_| {
            logout.emit(());
            if let Some(nav) = navigator.clone() {
                nav.push(&Route::Home);
            }
            || ()
        });
    }

    html! {
        <div class="page">
            <p class="empty-state">{"Signing you out…"}</p>
        </div>
    }
}
