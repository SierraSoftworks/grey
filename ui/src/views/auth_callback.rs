use yew::prelude::*;
use yew_router::prelude::*;

use crate::contexts::use_store;
// `Route` is only referenced from the wasm redirect below; it is unused in the SSR build.
#[cfg(not(feature = "ssr"))]
use crate::routes::Route;

/// The OIDC login callback page. The provider redirects the login popup here with `?code&state`;
/// the [`StoreProvider`](crate::contexts::StoreProvider) ancestor performs the actual code exchange
/// on mount — in a popup it hands the tokens back to the opener and closes the window, and on a
/// direct navigation it stores the session, after which this view sends the user home.
#[function_component(AuthCallback)]
pub fn auth_callback() -> Html {
    let store = use_store();
    let navigator = use_navigator();

    {
        let navigator = navigator.clone();
        // Once a session is established (the direct-navigation fallback), or if there is no callback
        // to process at all, return to the status page. In a popup the window closes itself via the
        // StoreProvider before this fires.
        use_effect_with(store.is_authenticated(), move |&authenticated| {
            #[cfg(feature = "wasm")]
            if (authenticated || !crate::auth::has_pending_callback())
                && let Some(nav) = navigator.clone()
            {
                nav.push(&Route::Home);
            }
            #[cfg(not(feature = "wasm"))]
            let _ = (authenticated, &navigator);
            || ()
        });
    }

    html! {
        <div class="page">
            <p class="empty-state">{"Completing sign-in…"}</p>
        </div>
    }
}
