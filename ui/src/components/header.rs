use super::cluster_status::ClusterStatus;
use crate::contexts::{use_auth, use_ui_config};
use crate::routes::Route;
use yew::prelude::*;
use yew_router::prelude::*;

#[function_component(Header)]
pub fn header() -> Html {
    let config_ctx = use_ui_config();
    let auth = use_auth();
    let menu_open = use_state(|| false);

    // Prefer the user's name, falling back to their email address.
    let user_display = auth
        .user
        .as_ref()
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
            <Link<Route> to={Route::Home} classes="header-brand">
                <img src={config_ctx.config.logo.clone()} alt="The company logo." />
                <span class="title">{&config_ctx.config.title}</span>
            </Link<Route>>

            <nav class="header-nav">
                <Link<Route> to={Route::Incidents} classes="nav-link">{"Incidents"}</Link<Route>>
                {
                    for config_ctx.config.links.iter().map(|link| {
                        html! {
                            <a href={link.url.clone()} class="nav-link" target="_blank" rel="noopener noreferrer">{&link.title}</a>
                        }
                    })
                }
            </nav>

            <div class="header-controls">
                if auth.is_authenticated() {
                    <ClusterStatus />
                }

                if auth.is_authenticated() {
                    // One control: shows the user, reveals a "Sign out" overlay on hover, and signs
                    // out when clicked.
                    <button class="user-chip" onclick={auth.logout.reform(|_| ())} title="Sign out">
                        <span class="user-chip__name">{ user_display.clone() }</span>
                        <span class="user-chip__signout" aria-hidden="true">{"Sign out"}</span>
                    </button>
                } else if auth.configured {
                    <button class="auth-button" onclick={auth.login.reform(|_| ())}>{"Sign in"}</button>
                }

                <button class="menu-toggle" onclick={toggle_menu}>
                    <div class="hamburger">
                        <span></span>
                        <span></span>
                        <span></span>
                    </div>
                </button>
            </div>
        </header>
    }
}
