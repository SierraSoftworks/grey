use yew::prelude::*;
use super::status_indicator::StatusIndicator;
use grey_api::UiConfig;

#[derive(Properties, PartialEq)]
pub struct HeaderProps {
    pub config: UiConfig,
    pub last_update: chrono::DateTime<chrono::Utc>,
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
                {if !props.config.links.is_empty() {
                    html! {
                        <nav class="header-nav">
                            {for props.config.links.iter().map(|link| {
                                html! {
                                    <a href={link.url.clone()} class="nav-link">{&link.title}</a>
                                }
                            })}
                        </nav>
                    }
                } else {
                    html! {}
                }}

                <StatusIndicator last_update={props.last_update} />
            </div>
        </header>
    }
}
