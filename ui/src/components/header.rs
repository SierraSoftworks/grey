use super::cluster_status::ClusterStatus;
use crate::contexts::use_store;
use crate::routes::Route;
use yew::prelude::*;
use yew_router::prelude::*;

#[function_component(Header)]
pub fn header() -> Html {
    let store = use_store();
    let menu_open = use_state(|| false);

    // Prefer the user's name, falling back to their email address.
    let user_display = store
        .user()
        .and_then(|u| u.name.clone().or_else(|| u.email.clone()))
        .unwrap_or_else(|| "Admin".to_string());

    let toggle_menu = {
        let menu_open = menu_open.clone();
        Callback::from(move |_| {
            menu_open.set(!*menu_open);
        })
    };

    let header_class = if *menu_open { "menu-open" } else { "" };

    html! {
        <header class={header_class}>
            <Link<Route> to={Route::Home} classes="header__brand">
                <img src={store.config().logo.clone()} alt="The company logo." />
                <span class="header__title">{&store.config().title}</span>
            </Link<Route>>

            <nav class="header__nav">
                <Link<Route> to={Route::Incidents} classes="header__nav-link">{"Incidents"}</Link<Route>>
                {
                    for store.config().links.iter().map(|link| {
                        html! {
                            <a href={link.url.clone()} class="header__nav-link" target="_blank" rel="noopener noreferrer">{&link.title}</a>
                        }
                    })
                }
            </nav>

            <div class="header__controls">
                if store.is_authenticated() {
                    <ClusterStatus />
                }

                if store.is_authenticated() {
                    // One control: shows the user, reveals a "Sign out" overlay on hover, and links
                    // to the logout route (which clears the session and returns home) when clicked.
                    <Link<Route> to={Route::AuthLogout} classes="user-chip">
                        <span class="user-chip__name">{ user_display.clone() }</span>
                        <span class="user-chip__signout" aria-hidden="true">{"Sign out"}</span>
                    </Link<Route>>
                } else if store.auth_configured() {
                    <button class="auth-button" onclick={store.login.reform(|_| ())}>{"Sign in"}</button>
                }

                <button class="header__menu-toggle" onclick={toggle_menu}>
                    <div class="header__hamburger">
                        <span></span>
                        <span></span>
                        <span></span>
                    </div>
                </button>
            </div>
        </header>
    }
}
