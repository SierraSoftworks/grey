//! A centred "nothing here" panel, used when a page has loaded but has no content to show (for
//! example, no incidents have ever been reported). Distinct from the inline `.empty-state` text and
//! from the loading [`crate::components::skeleton`] placeholders: this is a deliberate, calm
//! end-state rather than a transient one.

use yew::prelude::*;

use crate::components::icons::check_icon;

#[derive(Properties, PartialEq)]
pub struct EmptyStateProps {
    /// The headline (e.g. "No incidents reported").
    pub title: AttrValue,
    /// Optional supporting copy shown beneath the title.
    #[prop_or_default]
    pub children: Children,
}

#[function_component(EmptyState)]
pub fn empty_state(props: &EmptyStateProps) -> Html {
    html! {
        <div class="empty-panel">
            <div class="empty-panel__icon">{ check_icon() }</div>
            <h2 class="empty-panel__title">{ &props.title }</h2>
            if !props.children.is_empty() {
                <p class="empty-panel__body">{ for props.children.iter() }</p>
            }
        </div>
    }
}
