use crate::contexts::use_store;
use grey_api::{NoticeLevel, UiNotice};
use yew::prelude::*;

#[function_component(Timeline)]
pub fn timeline() -> Html {
    let store = use_store();

    if store.notices().is_empty() {
        return html! {};
    }

    html! {
        <div class="notices-timeline">
            <div class="notices-timeline__line"></div>
            {for store.notices().iter().map(|notice| {
                html! {
                    <Notice notice={notice.clone()} />
                }
            })}
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct NoticeProps {
    pub notice: UiNotice,
}

#[function_component(Notice)]
pub fn notice(props: &NoticeProps) -> Html {
    let level_class = match props.notice.level {
        Some(NoticeLevel::Ok) => "ok",
        Some(NoticeLevel::Warning) => "warning",
        Some(NoticeLevel::Error) => "error",
        None => "",
    };

    let timestamp_display = if let Some(timestamp) = props.notice.timestamp {
        format!("{}", timestamp.format("%Y-%m-%d %H:%M UTC"))
    } else {
        String::new()
    };

    html! {
        <div class={format!("notices-timeline__item {}", level_class)}>
            <div class="notices-timeline__dot-container">
                <div class={format!("notices-timeline__dot {}", level_class)}></div>
            </div>
            <div class="notices-timeline__content">
                <div class="notices-timeline__header">
                    <h3>{&props.notice.title}</h3>
                    <span class="notices-timeline__timestamp">{&timestamp_display}</span>
                </div>
                <p>{&props.notice.description}</p>
            </div>
        </div>
    }
}
