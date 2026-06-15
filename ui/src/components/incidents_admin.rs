//! Administrator-only incident management UI: the full (including hidden) incident list with
//! create / edit / add-update / hide / delete controls. Browser-only — it reads DOM input values
//! and performs authenticated mutations — so the whole module is gated to the `wasm` build.

use crate::api;
use crate::components::incidents_page::IncidentCard;
use chrono::{DateTime, NaiveDateTime, Utc};
use grey_api::{Incident, IncidentInput, IncidentStatus, NewIncidentUpdate};
use web_sys::{HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement};
use yew::prelude::*;

fn dt_to_input(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M").to_string()
}

fn parse_dt(value: &str) -> Option<DateTime<Utc>> {
    if value.is_empty() {
        return None;
    }
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M")
        .ok()
        .map(|naive| naive.and_utc())
}

fn status_from_str(value: &str) -> IncidentStatus {
    match value {
        "healthy" => IncidentStatus::Healthy,
        "degraded" => IncidentStatus::Degraded,
        "offline" => IncidentStatus::Offline,
        _ => IncidentStatus::Unknown,
    }
}

fn input_from_incident(incident: &Incident) -> IncidentInput {
    IncidentInput {
        title: incident.title.clone(),
        description: incident.description.clone(),
        start_time: incident.start_time,
        end_time: incident.end_time,
        detection_time: incident.detection_time,
        mitigation_time: incident.mitigation_time,
        affected_services: incident.affected_services.clone(),
        visible: incident.visible,
    }
}

fn bind_input(state: &UseStateHandle<String>) -> Callback<InputEvent> {
    let state = state.clone();
    Callback::from(move |e: InputEvent| {
        let el: HtmlInputElement = e.target_unchecked_into();
        state.set(el.value());
    })
}

fn bind_textarea(state: &UseStateHandle<String>) -> Callback<InputEvent> {
    let state = state.clone();
    Callback::from(move |e: InputEvent| {
        let el: HtmlTextAreaElement = e.target_unchecked_into();
        state.set(el.value());
    })
}

#[derive(Properties, PartialEq)]
pub struct AdminIncidentsProps {
    pub token: String,
}

#[function_component(AdminIncidents)]
pub fn admin_incidents(props: &AdminIncidentsProps) -> Html {
    let token = props.token.clone();
    let incidents = use_state(Vec::<Incident>::new);
    let error = use_state(|| Option::<String>::None);
    let creating = use_state(|| false);

    let refresh = {
        let token = token.clone();
        let incidents = incidents.clone();
        let error = error.clone();
        Callback::from(move |_: ()| {
            let token = token.clone();
            let incidents = incidents.clone();
            let error = error.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::list_incidents(&token).await {
                    Ok(list) => {
                        incidents.set(list);
                        error.set(None);
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
            });
        })
    };

    {
        let refresh = refresh.clone();
        use_effect_with(token.clone(), move |_| {
            refresh.emit(());
            || ()
        });
    }

    let on_new = {
        let creating = creating.clone();
        Callback::from(move |_| creating.set(true))
    };
    let on_cancel_new = {
        let creating = creating.clone();
        Callback::from(move |_: ()| creating.set(false))
    };
    let on_created = {
        let creating = creating.clone();
        let refresh = refresh.clone();
        Callback::from(move |_: ()| {
            creating.set(false);
            refresh.emit(());
        })
    };

    html! {
        <div class="content incidents-page incidents-admin">
            <div class="incidents-admin-header">
                <h1>{"Incidents"}</h1>
                if !*creating {
                    <button class="primary-button" onclick={on_new}>{"New incident"}</button>
                }
            </div>

            if let Some(err) = (*error).clone() {
                <p class="incidents-error">{err}</p>
            }

            if *creating {
                <IncidentForm
                    token={token.clone()}
                    incident={None::<Incident>}
                    on_saved={on_created}
                    on_cancel={on_cancel_new}
                />
            }

            if incidents.is_empty() {
                <p class="incidents-empty">{"No incidents yet. Create one to get started."}</p>
            } else {
                { for incidents.iter().map(|incident| html! {
                    <AdminIncidentCard
                        key={incident.id.clone()}
                        token={token.clone()}
                        incident={incident.clone()}
                        on_changed={refresh.clone()}
                    />
                }) }
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct AdminIncidentCardProps {
    token: String,
    incident: Incident,
    on_changed: Callback<()>,
}

#[function_component(AdminIncidentCard)]
fn admin_incident_card(props: &AdminIncidentCardProps) -> Html {
    let editing = use_state(|| false);
    let adding = use_state(|| false);

    let token = props.token.clone();
    let incident = props.incident.clone();

    let on_delete = {
        let token = token.clone();
        let id = incident.id.clone();
        let on_changed = props.on_changed.clone();
        Callback::from(move |_| {
            let token = token.clone();
            let id = id.clone();
            let on_changed = on_changed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if api::delete_incident(&token, &id).await.is_ok() {
                    on_changed.emit(());
                }
            });
        })
    };

    let on_toggle_visibility = {
        let token = token.clone();
        let incident = incident.clone();
        let on_changed = props.on_changed.clone();
        Callback::from(move |_| {
            let token = token.clone();
            let id = incident.id.clone();
            let mut input = input_from_incident(&incident);
            input.visible = !incident.visible;
            let on_changed = on_changed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if api::update_incident(&token, &id, &input).await.is_ok() {
                    on_changed.emit(());
                }
            });
        })
    };

    let toggle_edit = {
        let editing = editing.clone();
        Callback::from(move |_| editing.set(!*editing))
    };
    let toggle_add = {
        let adding = adding.clone();
        Callback::from(move |_| adding.set(!*adding))
    };

    let on_saved = {
        let editing = editing.clone();
        let on_changed = props.on_changed.clone();
        Callback::from(move |_: ()| {
            editing.set(false);
            on_changed.emit(());
        })
    };
    let on_added = {
        let adding = adding.clone();
        let on_changed = props.on_changed.clone();
        Callback::from(move |_: ()| {
            adding.set(false);
            on_changed.emit(());
        })
    };
    let cancel_edit = {
        let editing = editing.clone();
        Callback::from(move |_: ()| editing.set(false))
    };
    let cancel_add = {
        let adding = adding.clone();
        Callback::from(move |_: ()| adding.set(false))
    };

    let controls = html! {
        <div class="incident-admin-controls">
            <button onclick={toggle_edit}>{ if *editing { "Close editor" } else { "Edit" } }</button>
            <button onclick={toggle_add}>{ if *adding { "Close update" } else { "Add update" } }</button>
            <button onclick={on_toggle_visibility}>
                { if incident.visible { "Hide" } else { "Show" } }
            </button>
            <button class="danger" onclick={on_delete}>{"Delete"}</button>
        </div>
    };

    html! {
        <>
            <IncidentCard incident={incident.clone()} controls={controls} />
            if *editing {
                <IncidentForm
                    token={token.clone()}
                    incident={Some(incident.clone())}
                    on_saved={on_saved}
                    on_cancel={cancel_edit}
                />
            }
            if *adding {
                <UpdateForm
                    token={token.clone()}
                    incident_id={incident.id.clone()}
                    on_saved={on_added}
                    on_cancel={cancel_add}
                />
            }
        </>
    }
}

#[derive(Properties, PartialEq)]
struct IncidentFormProps {
    token: String,
    incident: Option<Incident>,
    on_saved: Callback<()>,
    on_cancel: Callback<()>,
}

#[function_component(IncidentForm)]
fn incident_form(props: &IncidentFormProps) -> Html {
    let existing = props.incident.clone();
    let is_edit = existing.is_some();

    let init_title = existing.as_ref().map(|i| i.title.clone()).unwrap_or_default();
    let init_description = existing.as_ref().map(|i| i.description.clone()).unwrap_or_default();
    let init_start = existing.as_ref().map(|i| dt_to_input(i.start_time)).unwrap_or_default();
    let init_end = existing.as_ref().and_then(|i| i.end_time).map(dt_to_input).unwrap_or_default();
    let init_detection = existing.as_ref().and_then(|i| i.detection_time).map(dt_to_input).unwrap_or_default();
    let init_mitigation = existing.as_ref().and_then(|i| i.mitigation_time).map(dt_to_input).unwrap_or_default();
    let init_services = existing.as_ref().map(|i| i.affected_services.join(", ")).unwrap_or_default();
    let init_visible = existing.as_ref().map(|i| i.visible).unwrap_or(true);

    let title = use_state(move || init_title);
    let description = use_state(move || init_description);
    let start = use_state(move || init_start);
    let end = use_state(move || init_end);
    let detection = use_state(move || init_detection);
    let mitigation = use_state(move || init_mitigation);
    let services = use_state(move || init_services);
    let visible = use_state(move || init_visible);
    let error = use_state(|| Option::<String>::None);
    let saving = use_state(|| false);

    let on_visible = {
        let visible = visible.clone();
        Callback::from(move |e: Event| {
            let el: HtmlInputElement = e.target_unchecked_into();
            visible.set(el.checked());
        })
    };

    let onsubmit = {
        let token = props.token.clone();
        let existing_id = existing.as_ref().map(|i| i.id.clone());
        let (title, description, start, end, detection, mitigation, services, visible) = (
            title.clone(),
            description.clone(),
            start.clone(),
            end.clone(),
            detection.clone(),
            mitigation.clone(),
            services.clone(),
            visible.clone(),
        );
        let error = error.clone();
        let saving = saving.clone();
        let on_saved = props.on_saved.clone();
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();

            let title_value = (*title).trim().to_string();
            if title_value.is_empty() {
                error.set(Some("A title is required.".into()));
                return;
            }

            let input = IncidentInput {
                title: title_value,
                description: (*description).clone(),
                start_time: parse_dt(&start).unwrap_or_else(Utc::now),
                end_time: parse_dt(&end),
                detection_time: parse_dt(&detection),
                mitigation_time: parse_dt(&mitigation),
                affected_services: (*services)
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                visible: *visible,
            };

            let token = token.clone();
            let existing_id = existing_id.clone();
            let error = error.clone();
            let saving = saving.clone();
            let on_saved = on_saved.clone();
            saving.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                let result = match existing_id {
                    Some(id) => api::update_incident(&token, &id, &input).await.map(|_| ()),
                    None => api::create_incident(&token, &input).await.map(|_| ()),
                };
                saving.set(false);
                match result {
                    Ok(()) => on_saved.emit(()),
                    Err(e) => error.set(Some(e.to_string())),
                }
            });
        })
    };

    let oncancel = {
        let on_cancel = props.on_cancel.clone();
        Callback::from(move |_| on_cancel.emit(()))
    };

    html! {
        <form class="incident-form" onsubmit={onsubmit}>
            <h3>{ if is_edit { "Edit incident" } else { "New incident" } }</h3>
            if let Some(err) = (*error).clone() {
                <p class="incidents-error">{err}</p>
            }
            <label>{"Title"}
                <input type="text" value={(*title).clone()} oninput={bind_input(&title)} />
            </label>
            <label>{"Description (markdown)"}
                <textarea rows="4" value={(*description).clone()} oninput={bind_textarea(&description)} />
            </label>
            <div class="incident-form-times">
                <label>{"Started (UTC)"}
                    <input type="datetime-local" value={(*start).clone()} oninput={bind_input(&start)} />
                </label>
                <label>{"Detected (UTC)"}
                    <input type="datetime-local" value={(*detection).clone()} oninput={bind_input(&detection)} />
                </label>
                <label>{"Mitigated (UTC)"}
                    <input type="datetime-local" value={(*mitigation).clone()} oninput={bind_input(&mitigation)} />
                </label>
                <label>{"Resolved (UTC)"}
                    <input type="datetime-local" value={(*end).clone()} oninput={bind_input(&end)} />
                </label>
            </div>
            <label>{"Affected services (comma separated)"}
                <input type="text" value={(*services).clone()} oninput={bind_input(&services)} />
            </label>
            <label class="checkbox">
                <input type="checkbox" checked={*visible} onchange={on_visible} />
                {"Visible to unauthenticated visitors"}
            </label>
            <div class="incident-form-actions">
                <button type="submit" class="primary-button" disabled={*saving}>
                    { if *saving { "Saving…" } else { "Save" } }
                </button>
                <button type="button" onclick={oncancel}>{"Cancel"}</button>
            </div>
        </form>
    }
}

#[derive(Properties, PartialEq)]
struct UpdateFormProps {
    token: String,
    incident_id: String,
    on_saved: Callback<()>,
    on_cancel: Callback<()>,
}

#[function_component(UpdateForm)]
fn update_form(props: &UpdateFormProps) -> Html {
    let status = use_state(|| "offline".to_string());
    let message = use_state(String::new);
    let error = use_state(|| Option::<String>::None);
    let saving = use_state(|| false);

    let on_status = {
        let status = status.clone();
        Callback::from(move |e: Event| {
            let el: HtmlSelectElement = e.target_unchecked_into();
            status.set(el.value());
        })
    };

    let onsubmit = {
        let token = props.token.clone();
        let id = props.incident_id.clone();
        let status = status.clone();
        let message = message.clone();
        let error = error.clone();
        let saving = saving.clone();
        let on_saved = props.on_saved.clone();
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let message_value = (*message).trim().to_string();
            if message_value.is_empty() {
                error.set(Some("A message is required.".into()));
                return;
            }
            let update = NewIncidentUpdate {
                status: status_from_str(&status),
                message: message_value,
                timestamp: None,
            };
            let token = token.clone();
            let id = id.clone();
            let error = error.clone();
            let saving = saving.clone();
            let on_saved = on_saved.clone();
            saving.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                let result = api::add_update(&token, &id, &update).await;
                saving.set(false);
                match result {
                    Ok(_) => on_saved.emit(()),
                    Err(e) => error.set(Some(e.to_string())),
                }
            });
        })
    };

    let oncancel = {
        let on_cancel = props.on_cancel.clone();
        Callback::from(move |_| on_cancel.emit(()))
    };

    html! {
        <form class="incident-form incident-update-form" onsubmit={onsubmit}>
            <h4>{"Add status update"}</h4>
            if let Some(err) = (*error).clone() {
                <p class="incidents-error">{err}</p>
            }
            <label>{"Status"}
                <select onchange={on_status}>
                    <option value="offline" selected={*status == "offline"}>{"Offline"}</option>
                    <option value="degraded" selected={*status == "degraded"}>{"Degraded"}</option>
                    <option value="healthy" selected={*status == "healthy"}>{"Healthy"}</option>
                    <option value="unknown" selected={*status == "unknown"}>{"Unknown"}</option>
                </select>
            </label>
            <label>{"Message (markdown)"}
                <textarea rows="3" value={(*message).clone()} oninput={bind_textarea(&message)} />
            </label>
            <div class="incident-form-actions">
                <button type="submit" class="primary-button" disabled={*saving}>
                    { if *saving { "Posting…" } else { "Post update" } }
                </button>
                <button type="button" onclick={oncancel}>{"Cancel"}</button>
            </div>
        </form>
    }
}
