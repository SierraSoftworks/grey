use yew::prelude::*;

#[derive(Copy, Clone, PartialEq)]
pub enum StatusLevel {
    Good,
    Warning,
    Error,
}

impl StatusLevel {
    fn class_name(&self) -> &'static str {
        match self {
            StatusLevel::Good => "good",
            StatusLevel::Warning => "warning",
            StatusLevel::Error => "error",
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct StatusProps {
    pub status: StatusLevel,
    pub text: String,
}

#[function_component(Status)]
pub fn status(props: &StatusProps) -> Html {
    html! {
        <div class={format!("status-indicator {}", props.status.class_name())}>
            <div class="status-dot"></div>
            <span class="status-text">{&props.text}</span>
        </div>
    }
}