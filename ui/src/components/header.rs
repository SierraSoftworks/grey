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

    html! {
        <header>
            <div class="header-brand">
                <img src={config_ctx.config.logo.clone()} alt="The company logo." />
                <span class="title">{&config_ctx.config.title}</span>
            </div>
            <div class="header-right">
                if config_ctx.config.links.len() > 0 {
                    <nav class="header-nav">
                    {
                        for config_ctx.config.links.iter().map(|link| {
                            html! {
                                <a href={link.url.clone()} class="nav-link">{&link.title}</a>
                            }
                        })
                    }
                    </nav>
                }

                <Status status={props.status} text={props.status_text.clone()} />
            </div>
        </header>
    }
}
