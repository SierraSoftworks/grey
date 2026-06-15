use super::cluster_status::ClusterStatus;
use super::status::{Status, StatusLevel};
use crate::contexts::use_ui_config;
use crate::routes::Route;
use yew::prelude::*;
use yew_router::prelude::*;

#[derive(Properties, PartialEq)]
pub struct HeaderProps {
    pub status: StatusLevel,
    pub status_text: String,
}

#[function_component(Header)]
pub fn header(props: &HeaderProps) -> Html {
    let config_ctx = use_ui_config();
    let menu_open = use_state(|| false);

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
                <ClusterStatus />
                <Status status={props.status} text={props.status_text.clone()} />

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
