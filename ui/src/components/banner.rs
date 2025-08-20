use yew::prelude::*;

#[derive(Clone, PartialEq)]
pub enum BannerKind {
    Ok,
    Warning,
    Error,
}

impl std::fmt::Display for BannerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BannerKind::Ok => write!(f, "ok"),
            BannerKind::Warning => write!(f, "warn"),
            BannerKind::Error => write!(f, "error"),
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct BannerProps {
    pub kind: BannerKind,
    pub text: String,
}

#[function_component(Banner)]
pub fn banner(props: &BannerProps) -> Html {
    let kind_str = props.kind.to_string();

    html! {
        <div class={format!("section fill {}", kind_str)}>
            <span class={format!("status {}", kind_str)}>{&props.text}</span>
        </div>
    }
}
