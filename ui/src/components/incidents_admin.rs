//! Administrator-only incident management UI: the full (including hidden) incident list with
//! create / edit / add-update / delete controls. Browser-only — it reads DOM input values and
//! performs authenticated mutations — so the whole module is gated to the `wasm` build.

use crate::api;
use crate::components::incidents_timeline::{IncidentBlock, active_summary};
use crate::contexts::use_probes;
use chrono::{DateTime, NaiveDateTime, Utc};
use grey_api::{Impact, Incident, IncidentInput, NewIncidentUpdate};
use std::collections::BTreeSet;
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

fn impact_from_str(value: &str) -> Impact {
    match value {
        "offline" => Impact::Offline,
        "degraded" => Impact::Degraded,
        "none" => Impact::None,
        _ => Impact::Hidden,
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
    // Which incident (if any) currently has its editor open. Lifting this here lets "New incident"
    // create a draft and immediately open its editor.
    let editing = use_state(|| Option::<String>::None);

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

    // "New incident" saves a blank draft (started = now, no updates so it stays hidden) and opens
    // its editor immediately.
    let on_new = {
        let token = token.clone();
        let incidents = incidents.clone();
        let editing = editing.clone();
        let error = error.clone();
        Callback::from(move |_| {
            let token = token.clone();
            let incidents = incidents.clone();
            let editing = editing.clone();
            let error = error.clone();
            let draft = IncidentInput {
                title: "Untitled incident".to_string(),
                description: String::new(),
                start_time: Utc::now(),
                end_time: None,
                affected_services: vec![],
            };
            wasm_bindgen_futures::spawn_local(async move {
                match api::create_incident(&token, &draft).await {
                    Ok(created) => {
                        editing.set(Some(created.id.clone()));
                        match api::list_incidents(&token).await {
                            Ok(list) => {
                                incidents.set(list);
                                error.set(None);
                            }
                            Err(e) => error.set(Some(e.to_string())),
                        }
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
            });
        })
    };

    let (header_class, header_text) = active_summary(&incidents);

    html! {
        <div class="content incidents-section incidents-admin">
            <div class={classes!("section", "fill", header_class)}>
                <span class={classes!("status", header_class)}>{header_text}</span>
                <button class="primary-button" onclick={on_new}>{"New incident"}</button>
            </div>

            if let Some(err) = (*error).clone() {
                <div class="section"><p class="incidents-error">{err}</p></div>
            }

            if incidents.is_empty() {
                <div class="section"><p class="incidents-empty">{"No incidents yet. Create one to get started."}</p></div>
            } else {
                { for incidents.iter().map(|incident| {
                    let is_editing = editing.as_ref() == Some(&incident.id);
                    html! {
                        <AdminIncidentCard
                            key={incident.id.clone()}
                            token={token.clone()}
                            incident={incident.clone()}
                            editing={is_editing}
                            set_editing={editing.setter()}
                            on_changed={refresh.clone()}
                        />
                    }
                }) }
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct AdminIncidentCardProps {
    token: String,
    incident: Incident,
    editing: bool,
    set_editing: UseStateSetter<Option<String>>,
    on_changed: Callback<()>,
}

#[function_component(AdminIncidentCard)]
fn admin_incident_card(props: &AdminIncidentCardProps) -> Html {
    let adding = use_state(|| false);

    let token = props.token.clone();
    let incident = props.incident.clone();
    let id = incident.id.clone();

    let on_delete = {
        let token = token.clone();
        let id = id.clone();
        let on_changed = props.on_changed.clone();
        let set_editing = props.set_editing.clone();
        Callback::from(move |_| {
            let token = token.clone();
            let id = id.clone();
            let on_changed = on_changed.clone();
            let set_editing = set_editing.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if api::delete_incident(&token, &id).await.is_ok() {
                    set_editing.set(None);
                    on_changed.emit(());
                }
            });
        })
    };

    let toggle_edit = {
        let set_editing = props.set_editing.clone();
        let id = id.clone();
        let editing = props.editing;
        Callback::from(move |_| {
            set_editing.set(if editing { None } else { Some(id.clone()) });
        })
    };
    let toggle_add = {
        let adding = adding.clone();
        Callback::from(move |_| adding.set(!*adding))
    };

    let on_saved = {
        let set_editing = props.set_editing.clone();
        let on_changed = props.on_changed.clone();
        Callback::from(move |_: ()| {
            set_editing.set(None);
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
        let set_editing = props.set_editing.clone();
        Callback::from(move |_: ()| set_editing.set(None))
    };
    let cancel_add = {
        let adding = adding.clone();
        Callback::from(move |_: ()| adding.set(false))
    };

    let controls = html! {
        <div class="incident-admin-controls">
            <button onclick={toggle_edit}>{ if props.editing { "Close editor" } else { "Edit" } }</button>
            <button onclick={toggle_add}>{ if *adding { "Close update" } else { "Add update" } }</button>
            <button class="danger" onclick={on_delete}>{"Delete"}</button>
        </div>
    };

    html! {
        <>
            <IncidentBlock incident={incident.clone()} controls={controls} />
            if props.editing {
                <IncidentForm
                    token={token.clone()}
                    incident={incident.clone()}
                    on_saved={on_saved}
                    on_cancel={cancel_edit}
                />
            }
            if *adding {
                <UpdateForm
                    token={token.clone()}
                    incident_id={id.clone()}
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
    incident: Incident,
    on_saved: Callback<()>,
    on_cancel: Callback<()>,
}

#[function_component(IncidentForm)]
fn incident_form(props: &IncidentFormProps) -> Html {
    let incident = &props.incident;

    let title = use_state(|| incident.title.clone());
    let description = use_state(|| incident.description.clone());
    let start = use_state(|| dt_to_input(incident.start_time));
    let end = use_state(|| incident.end_time.map(dt_to_input).unwrap_or_default());
    let services = use_state(|| incident.affected_services.clone());
    let error = use_state(|| Option::<String>::None);
    let saving = use_state(|| false);

    // Suggest known service tags and probe names for the affected-services autocomplete.
    let probes_ctx = use_probes();
    let suggestions: Vec<String> = {
        let mut set = BTreeSet::new();
        for probe in &probes_ctx.probes {
            if let Some(service) = probe.tags.get("service") {
                if !service.is_empty() {
                    set.insert(service.clone());
                }
            }
            set.insert(probe.name.clone());
        }
        set.into_iter().collect()
    };

    let on_services_change = {
        let services = services.clone();
        Callback::from(move |next: Vec<String>| services.set(next))
    };

    let onsubmit = {
        let token = props.token.clone();
        let id = incident.id.clone();
        let (title, description, start, end, services) = (
            title.clone(),
            description.clone(),
            start.clone(),
            end.clone(),
            services.clone(),
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
                affected_services: (*services).clone(),
            };

            let token = token.clone();
            let id = id.clone();
            let error = error.clone();
            let saving = saving.clone();
            let on_saved = on_saved.clone();
            saving.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                let result = api::update_incident(&token, &id, &input).await;
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
        <form class="incident-form" onsubmit={onsubmit}>
            <h3>{"Edit incident"}</h3>
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
                <label>{"Ended (UTC)"}
                    <input type="datetime-local" value={(*end).clone()} oninput={bind_input(&end)} />
                </label>
            </div>
            <label>{"Affected services"}
                <AffectedServicesInput
                    selected={(*services).clone()}
                    suggestions={suggestions}
                    on_change={on_services_change}
                />
            </label>
            <p class="incident-form-hint">
                {"Set the incident's impact by posting updates (Offline, Degraded, Operational, or Hidden). An incident with no updates stays a hidden draft."}
            </p>
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
struct AffectedServicesInputProps {
    selected: Vec<String>,
    suggestions: Vec<String>,
    on_change: Callback<Vec<String>>,
}

/// A type-to-filter, click-to-add autocomplete for affected services, modelled on automate's filter
/// list. Selected services show as removable chips; matching suggestions appear in a dropdown.
#[function_component(AffectedServicesInput)]
fn affected_services_input(props: &AffectedServicesInputProps) -> Html {
    let query = use_state(String::new);
    let selected = props.selected.clone();

    let needle = query.to_lowercase();
    let matches: Vec<String> = props
        .suggestions
        .iter()
        .filter(|s| !selected.iter().any(|sel| sel.eq_ignore_ascii_case(s)))
        .filter(|s| needle.is_empty() || s.to_lowercase().contains(&needle))
        .take(8)
        .cloned()
        .collect();

    let add = {
        let on_change = props.on_change.clone();
        let selected = selected.clone();
        let query = query.clone();
        Callback::from(move |value: String| {
            let value = value.trim().to_string();
            query.set(String::new());
            if value.is_empty() || selected.iter().any(|s| s.eq_ignore_ascii_case(&value)) {
                return;
            }
            let mut next = selected.clone();
            next.push(value);
            on_change.emit(next);
        })
    };

    let remove = {
        let on_change = props.on_change.clone();
        let selected = selected.clone();
        Callback::from(move |value: String| {
            on_change.emit(selected.iter().filter(|s| **s != value).cloned().collect());
        })
    };

    let oninput = {
        let query = query.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlInputElement = e.target_unchecked_into();
            query.set(el.value());
        })
    };

    let onkeydown = {
        let add = add.clone();
        let query = query.clone();
        Callback::from(move |e: KeyboardEvent| {
            if e.key() == "Enter" {
                e.prevent_default();
                add.emit((*query).clone());
            }
        })
    };

    let show_dropdown = !needle.is_empty() && !matches.is_empty();

    html! {
        <div class="services-autocomplete">
            if !selected.is_empty() {
                <div class="services-chips">
                    { for selected.iter().map(|service| {
                        let service = service.clone();
                        let onclick = {
                            let remove = remove.clone();
                            let service = service.clone();
                            Callback::from(move |_| remove.emit(service.clone()))
                        };
                        html! {
                            <span class="service-chip">
                                { &service }
                                <button type="button" class="service-chip-remove" onclick={onclick}>{"×"}</button>
                            </span>
                        }
                    }) }
                </div>
            }
            <div class="services-input-wrap">
                <input
                    type="text"
                    placeholder="Add an affected service…"
                    value={(*query).clone()}
                    oninput={oninput}
                    onkeydown={onkeydown}
                />
                if show_dropdown {
                    <ul class="services-dropdown">
                        { for matches.iter().map(|suggestion| {
                            let suggestion = suggestion.clone();
                            let onclick = {
                                let add = add.clone();
                                let suggestion = suggestion.clone();
                                Callback::from(move |_| add.emit(suggestion.clone()))
                            };
                            html! {
                                <li>
                                    <button type="button" class="services-option" onclick={onclick}>
                                        { &suggestion }
                                    </button>
                                </li>
                            }
                        }) }
                    </ul>
                }
            </div>
        </div>
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
    let impact = use_state(|| "offline".to_string());
    let message = use_state(String::new);
    let error = use_state(|| Option::<String>::None);
    let saving = use_state(|| false);

    let on_impact = {
        let impact = impact.clone();
        Callback::from(move |e: Event| {
            let el: HtmlSelectElement = e.target_unchecked_into();
            impact.set(el.value());
        })
    };

    let onsubmit = {
        let token = props.token.clone();
        let id = props.incident_id.clone();
        let impact = impact.clone();
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
                impact: impact_from_str(&impact),
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
            <h4>{"Add update"}</h4>
            if let Some(err) = (*error).clone() {
                <p class="incidents-error">{err}</p>
            }
            <label>{"Impact"}
                <select onchange={on_impact}>
                    <option value="offline" selected={*impact == "offline"}>{"Offline"}</option>
                    <option value="degraded" selected={*impact == "degraded"}>{"Degraded"}</option>
                    <option value="none" selected={*impact == "none"}>{"Operational (no impact)"}</option>
                    <option value="hidden" selected={*impact == "hidden"}>{"Hidden"}</option>
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
