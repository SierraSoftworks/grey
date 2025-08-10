use super::status::{Status, StatusLevel};
use grey_api::UiConfig;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct HeaderProps {
    pub config: UiConfig,
    pub status: StatusLevel,
    pub status_text: String,
}

#[function_component(Header)]
pub fn header(props: &HeaderProps) -> Html {
    html! {
        <header>
            <div class="header-brand">
                <img src={props.config.logo.clone()} alt="The company logo." />
                <span class="title">{&props.config.title}</span>
            </div>
            <div class="header-right">
                if props.config.links.len() > 0 {
                    <nav class="header-nav">
                    {
                        for props.config.links.iter().map(|link| {
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
