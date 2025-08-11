use yew::prelude::*;
use grey_api::{UiNotice, NoticeLevel};

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
        format!("{}", timestamp.format("%Y-%m-%d\n%H:%M UTC"))
    } else {
        String::new()
    };

    html! {
        <div class={format!("timeline-item {}", level_class)}>
            <div class="timeline-timestamp">
                {if !timestamp_display.is_empty() {
                    html! { <span class="notice-timestamp">{timestamp_display}</span> }
                } else {
                    html! {}
                }}
            </div>
            <div class="timeline-dot-container">
                <div class={format!("timeline-dot {}", level_class)}></div>
            </div>
            <div class="timeline-content">
                <div class="notice-header">
                    <h3>{&props.notice.title}</h3>
                </div>
                <p>{&props.notice.description}</p>
            </div>
        </div>
    }
}
