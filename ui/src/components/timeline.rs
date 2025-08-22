use crate::contexts::use_notices;
use grey_api::{NoticeLevel, UiNotice};
use yew::prelude::*;

#[function_component(Timeline)]
pub fn timeline() -> Html {
    let notices_ctx = use_notices();

    if notices_ctx.notices.is_empty() {
        return html! {};
    }

    html! {
        <div class="notices-timeline">
            <div class="timeline-line"></div>
            {for notices_ctx.notices.iter().map(|notice| {
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
        <div class={format!("timeline-item {}", level_class)}>
            <div class="timeline-dot-container">
                <div class={format!("timeline-dot {}", level_class)}></div>
            </div>
            <div class="timeline-content">
                <div class="notice-header">
                    <h3>{&props.notice.title}</h3>
                    <span class="notice-timestamp">{&timestamp_display}</span>
                </div>
                <p>{&props.notice.description}</p>
            </div>
        </div>
    }
}
