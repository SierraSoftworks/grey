use yew::prelude::*;

use crate::contexts::use_auth;

/// The `/incidents/new` page (admin only): a title plus the incident's opening update (impact +
/// message). Saving creates the incident and navigates to its page.
#[function_component(NewIncident)]
pub fn new_incident() -> Html {
    let auth = use_auth();

    if !auth.is_authenticated() {
        return html! {
            <div class="incidents-page">
                <h1>{"New incident"}</h1>
                <p class="incidents-empty">{"Sign in as an administrator to create an incident."}</p>
            </div>
        };
    }

    #[cfg(feature = "wasm")]
    if let Some(token) = auth.token.clone() {
        return html! { <NewIncidentForm token={token} /> };
    }

    html! {}
}

#[cfg(feature = "wasm")]
#[derive(Properties, PartialEq)]
struct NewIncidentFormProps {
    token: String,
}

#[cfg(feature = "wasm")]
#[function_component(NewIncidentForm)]
fn new_incident_form(props: &NewIncidentFormProps) -> Html {
    use crate::routes::Route;
    use web_sys::{HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement};
    use yew_router::prelude::*;

    let title = use_state(String::new);
    let impact = use_state(|| "offline".to_string());
    let message = use_state(String::new);
    let error = use_state(|| Option::<String>::None);
    let saving = use_state(|| false);
    let navigator = use_navigator();
    // The shared in-memory list, so a newly created incident shows up at once.
    let incidents = crate::contexts::use_incidents();

    let on_title = {
        let title = title.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlInputElement = e.target_unchecked_into();
            title.set(el.value());
        })
    };
    let on_impact = {
        let impact = impact.clone();
        Callback::from(move |e: Event| {
            let el: HtmlSelectElement = e.target_unchecked_into();
            impact.set(el.value());
        })
    };
    let on_message = {
        let message = message.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlTextAreaElement = e.target_unchecked_into();
            message.set(el.value());
        })
    };

    let onsubmit = {
        let token = props.token.clone();
        let (title, impact, message) = (title.clone(), impact.clone(), message.clone());
        let error = error.clone();
        let saving = saving.clone();
        let navigator = navigator.clone();
        let upsert = incidents.upsert.clone();
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let title_value = (*title).trim().to_string();
            let message_value = (*message).trim().to_string();
            if title_value.is_empty() || message_value.is_empty() {
                error.set(Some("A title and an initial update message are required.".into()));
                return;
            }
            let input = grey_api::CreateIncident {
                title: title_value,
                impact: crate::views::parse_impact(&impact),
                message: message_value,
            };
            let token = token.clone();
            let error = error.clone();
            let saving = saving.clone();
            let navigator = navigator.clone();
            let upsert = upsert.clone();
            saving.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                match crate::api::create_incident(&token, &input).await {
                    Ok(created) => {
                        // Reflect the new incident in the shared list before navigating to it.
                        upsert.emit(created.clone());
                        if let Some(nav) = navigator {
                            nav.push(&Route::Incident { id: created.id.to_string() });
                        }
                    }
                    Err(e) => {
                        saving.set(false);
                        error.set(Some(e.to_string()));
                    }
                }
            });
        })
    };

    html! {
        <div class="incidents-page">
            <h1>{"New incident"}</h1>
            <form class="incident-form" onsubmit={onsubmit}>
                if let Some(err) = (*error).clone() {
                    <p class="incidents-error">{err}</p>
                }
                <label>{"Title"}
                    <input type="text" value={(*title).clone()} oninput={on_title} />
                </label>
                <label>{"Initial impact"}
                    <select onchange={on_impact}>
                        <option value="offline" selected={*impact == "offline"}>{"Offline"}</option>
                        <option value="degraded" selected={*impact == "degraded"}>{"Degraded"}</option>
                        <option value="none" selected={*impact == "none"}>{"Operational (no impact)"}</option>
                        <option value="hidden" selected={*impact == "hidden"}>{"Hidden (draft)"}</option>
                    </select>
                </label>
                <label>{"Initial update (markdown)"}
                    <textarea rows="4" value={(*message).clone()} oninput={on_message} />
                </label>
                <div class="incident-form-actions">
                    <button type="submit" class="primary-button" disabled={*saving}>
                        { if *saving { "Creating…" } else { "Create incident" } }
                    </button>
                </div>
            </form>
        </div>
    }
}
