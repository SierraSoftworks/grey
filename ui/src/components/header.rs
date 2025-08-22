use super::status::{Status, StatusLevel};
use crate::contexts::use_ui_config;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct HeaderProps {
    pub status: StatusLevel,
    pub status_text: String,
}

#[function_component(Header)]
pub fn header(props: &HeaderProps) -> Html {
    let config_ctx = use_ui_config();
    let menu_open = use_state(|| false);

    let has_links = !config_ctx.config.links.is_empty();

    let toggle_menu = {
        let menu_open = menu_open.clone();
        Callback::from(move |_| {
            menu_open.set(!*menu_open);
        })
    };

    let header_class = if *menu_open && has_links {
        "menu-open"
    } else {
        ""
    };

    html! {
        <header class={header_class}>
            <div class="header-brand">
                <img src={config_ctx.config.logo.clone()} alt="The company logo." />
                <span class="title">{&config_ctx.config.title}</span>
            </div>

            if has_links {
                <nav class="header-nav">
                {
                    for config_ctx.config.links.iter().map(|link| {
                        html! {
                            <a href={link.url.clone()} class="nav-link" target="_blank" rel="noopener noreferrer">{&link.title}</a>
                        }
                    })
                }
                </nav>
            }

            <div class="header-controls">
                <Status status={props.status} text={props.status_text.clone()} />

                if has_links {
                    <button class="menu-toggle" onclick={toggle_menu}>
                        <div class="hamburger">
                            <span></span>
                            <span></span>
                            <span></span>
                        </div>
                    </button>
                }
            </div>
        </header>
    }
}
