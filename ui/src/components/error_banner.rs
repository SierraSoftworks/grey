use yew::prelude::*;

use crate::components::icons::{close_icon, warning_icon};
use crate::contexts::use_store;

/// A dismissible banner pinned to the top of the page whenever the store is holding an error (for
/// example a failed background refresh). It shows the error's message, any advice the agent attached
/// for resolving it, and a close button that clears the error from the store.
#[function_component(ErrorBanner)]
pub fn error_banner() -> Html {
    let store = use_store();

    let Some(error) = store.error().cloned() else {
        return html! {};
    };

    let on_dismiss = store.clear_error.reform(|_| ());

    html! {
        <div class="error-banner" role="alert">
            <div class="error-banner__icon">{ warning_icon() }</div>
            <div class="error-banner__body">
                <p class="error-banner__message">{ &error.message }</p>
                if !error.advice.is_empty() {
                    <ul class="error-banner__advice">
                        { for error.advice.iter().map(|advice| html! { <li>{ advice }</li> }) }
                    </ul>
                }
            </div>
            <button
                type="button"
                class="error-banner__close"
                aria-label="Dismiss error"
                onclick={on_dismiss}
            >
                { close_icon() }
            </button>
        </div>
    }
}
