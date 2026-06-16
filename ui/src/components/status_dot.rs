use yew::prelude::*;

/// A small, coloured status dot — the same visual used in the probe-history tooltips. The `class`
/// is the colour modifier (`ok`, `warn`, `error`, `unknown`, or `draft`), matching the status
/// colour classes used across the UI.
#[derive(Properties, PartialEq)]
pub struct StatusDotProps {
    pub class: AttrValue,
}

#[function_component(StatusDot)]
pub fn status_dot(props: &StatusDotProps) -> Html {
    html! {
        <span class={classes!("status-dot-indicator", props.class.to_string())}></span>
    }
}
