use yew::prelude::*;

/// A small, coloured status dot used across the UI. The `class` is the colour modifier (`ok`,
/// `warn`, `error`, `running`, `unknown`, or `draft`), matching the status colour classes used
/// elsewhere.
/// `active` toggles the pulse animation, and `size` controls the diameter in pixels.
#[derive(Properties, PartialEq)]
pub struct StatusDotProps {
    pub class: AttrValue,
    /// When `true`, the dot pulses to draw attention (e.g. a live probe header).
    #[prop_or_default]
    pub active: bool,
    /// Diameter of the dot in pixels.
    #[prop_or(8)]
    pub size: usize,
}

#[function_component(StatusDot)]
pub fn status_dot(props: &StatusDotProps) -> Html {
    let mut class = classes!("status-dot", props.class.to_string());
    if props.active {
        class.push("active");
    }

    let style = format!("width:{0}px;height:{0}px", props.size);

    html! {
        <span class={class} {style}></span>
    }
}
