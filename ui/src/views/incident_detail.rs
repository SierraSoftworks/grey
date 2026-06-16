use grey_api::Identifier;
use yew::prelude::*;

use crate::components::{IncidentBlock, StatusDot};
use crate::contexts::{use_auth, use_incidents};

/// The `/incidents/{id}` page. Public visitors see the read-only incident; signed-in administrators
/// get an inline editor (click any field to change it; a save control appears once there are
/// unsaved changes) plus controls to add/remove updates and delete the incident.
#[derive(Properties, PartialEq)]
pub struct IncidentDetailProps {
    pub id: String,
}

#[function_component(IncidentDetail)]
pub fn incident_detail(props: &IncidentDetailProps) -> Html {
    let auth = use_auth();
    let incidents_ctx = use_incidents();

    #[cfg(feature = "wasm")]
    if auth.is_authenticated() {
        if let Some(token) = auth.token.clone() {
            return html! { <AdminIncidentDetail id={props.id.clone()} token={token} /> };
        }
    }
    #[cfg(not(feature = "wasm"))]
    let _ = &auth;

    let wanted = Identifier::parse(&props.id);
    let incident = incidents_ctx.incidents.iter().find(|i| Some(i.id) == wanted).cloned();

    html! {
        <div class="incidents-page">
            if let Some(incident) = incident {
                <IncidentBlock incident={incident} />
            } else {
                <h1>{"Incident not found"}</h1>
                <p class="incidents-empty">
                    {"This incident does not exist or is not publicly visible."}
                </p>
            }
        </div>
    }
}

#[cfg(feature = "wasm")]
#[derive(Properties, PartialEq)]
struct AdminIncidentDetailProps {
    id: String,
    token: String,
}

#[cfg(feature = "wasm")]
#[function_component(AdminIncidentDetail)]
fn admin_incident_detail(props: &AdminIncidentDetailProps) -> Html {
    use crate::components::icons::{check_icon, edit_icon, save_icon};
    use crate::components::markdown::render_markdown;
    use crate::routes::Route;
    use crate::styles::impact_class;
    use chrono::Utc;
    use grey_api::{Impact, IncidentEdit, IncidentUpdate};
    use web_sys::{HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement};
    use yew_router::prelude::*;

    // The canonical incident (with its version) plus the editable draft fields.
    let loaded = use_state(|| Option::<grey_api::Incident>::None);
    let title = use_state(String::new);
    let updates = use_state(Vec::<IncidentUpdate>::new);
    let error = use_state(|| Option::<String>::None);
    let saving = use_state(|| false);
    // Which posted update (by index) currently has its message open in a textarea; the rest render
    // their message as markdown.
    let editing = use_state(|| Option::<usize>::None);
    let navigator = use_navigator();
    // The shared in-memory list, so saves/deletes are reflected everywhere without a refetch.
    let incidents = crate::contexts::use_incidents();

    // Load the incident (including hidden) once.
    {
        let token = props.token.clone();
        let id = props.id.clone();
        let loaded = loaded.clone();
        let title = title.clone();
        let updates = updates.clone();
        let error = error.clone();
        use_effect_with((props.token.clone(), props.id.clone()), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                match crate::api::get_incident(&token, &id).await {
                    Ok(incident) => {
                        title.set(incident.title.clone());
                        updates.set(incident.sorted_updates().into_iter().cloned().collect());
                        loaded.set(Some(incident));
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
            });
            || ()
        });
    }

    let Some(current) = (*loaded).clone() else {
        return html! {
            <div class="incidents-page">
                if let Some(err) = (*error).clone() {
                    <p class="incidents-error">{err}</p>
                } else {
                    <p class="incidents-empty">{"Loading…"}</p>
                }
            </div>
        };
    };

    // The unchanged baseline, in the same (sorted) order as the editable list, so reordering by the
    // server's storage order never reads as a spurious change.
    let baseline: Vec<IncidentUpdate> = current.sorted_updates().into_iter().cloned().collect();
    let dirty = current.title != *title || baseline != *updates;

    let on_title = {
        let title = title.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlInputElement = e.target_unchecked_into();
            title.set(el.value());
        })
    };

    let on_update_impact = |index: usize| {
        let updates = updates.clone();
        Callback::from(move |e: Event| {
            let el: HtmlSelectElement = e.target_unchecked_into();
            let mut next = (*updates).clone();
            if let Some(u) = next.get_mut(index) {
                u.impact = el.value().parse().unwrap_or_default();
            }
            updates.set(next);
        })
    };
    let on_update_message = |index: usize| {
        let updates = updates.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlTextAreaElement = e.target_unchecked_into();
            let mut next = (*updates).clone();
            if let Some(u) = next.get_mut(index) {
                u.message = el.value();
            }
            updates.set(next);
        })
    };
    let on_remove_update = |index: usize| {
        let updates = updates.clone();
        Callback::from(move |_| {
            let mut next = (*updates).clone();
            if index < next.len() {
                next.remove(index);
            }
            updates.set(next);
        })
    };
    let on_add_update = {
        let updates = updates.clone();
        Callback::from(move |_| {
            let mut next = (*updates).clone();
            next.push(IncidentUpdate {
                impact: Impact::Offline,
                timestamp: Utc::now(),
                message: String::new(),
            });
            updates.set(next);
        })
    };

    let on_save = {
        let token = props.token.clone();
        let id = current.id.to_string();
        let version = current.version;
        let title = title.clone();
        let updates = updates.clone();
        let loaded = loaded.clone();
        let error = error.clone();
        let saving = saving.clone();
        let editing = editing.clone();
        let upsert = incidents.upsert.clone();
        Callback::from(move |_| {
            let edit = IncidentEdit {
                title: (*title).trim().to_string(),
                updates: (*updates).clone(),
            };
            let token = token.clone();
            let id = id.clone();
            let loaded = loaded.clone();
            let title = title.clone();
            let updates = updates.clone();
            let error = error.clone();
            let saving = saving.clone();
            let editing = editing.clone();
            let upsert = upsert.clone();
            saving.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                let result = crate::api::replace_incident(&token, &id, version, &edit).await;
                saving.set(false);
                match result {
                    Ok(incident) => {
                        title.set(incident.title.clone());
                        updates.set(incident.sorted_updates().into_iter().cloned().collect());
                        // Reflect the saved incident in the shared list immediately.
                        upsert.emit(incident.clone());
                        loaded.set(Some(incident));
                        error.set(None);
                        editing.set(None);
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
            });
        })
    };

    let on_delete = {
        let token = props.token.clone();
        let id = current.id.to_string();
        let id_value = current.id;
        let navigator = navigator.clone();
        let error = error.clone();
        let remove = incidents.remove.clone();
        Callback::from(move |_| {
            let token = token.clone();
            let id = id.clone();
            let navigator = navigator.clone();
            let error = error.clone();
            let remove = remove.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match crate::api::delete_incident(&token, &id).await {
                    Ok(()) => {
                        // Drop it from the shared list before leaving the page.
                        remove.emit(id_value);
                        if let Some(nav) = navigator {
                            nav.push(&Route::Incidents);
                        }
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
            });
        })
    };

    let status_class = impact_class(current.current_impact());

    html! {
        <div class="incidents-page incident-edit">
            <article class="incident-block">
                <div class="incident-block-header">
                    <div class="incident-title-block">
                        <input
                            class="incident-title-input"
                            type="text"
                            value={(*title).clone()}
                            oninput={on_title}
                        />
                        <span class="incident-id">{format!("#{}", current.id)}</span>
                    </div>
                    if dirty {
                        <button
                            class={classes!("save-icon", (*saving).then_some("saving"))}
                            title="Save changes"
                            disabled={*saving}
                            onclick={on_save}
                        >
                            { save_icon() }
                        </button>
                    }
                </div>

                if let Some(err) = (*error).clone() {
                    <p class="incidents-error">{err}</p>
                }

                <ul class="incident-timeline editing">
                    { for (*updates).iter().enumerate().rev().map(|(i, update)| {
                        let class = impact_class(update.impact);
                        // An update is "posted" once it is part of the loaded incident: its impact then
                        // becomes fixed and only its message stays editable. New (unsaved) updates keep
                        // full control so the impact can still be chosen before the first save.
                        let posted = current.updates.iter().any(|u| u.timestamp == update.timestamp);
                        let is_editing = *editing == Some(i);
                        html! {
                            <li class="timeline-item">
                                <div class="timeline-rail">
                                    <span class={classes!("timeline-circle", class)}></span>
                                    <span class={classes!("timeline-tail", class)}></span>
                                </div>
                                <div class="timeline-body">
                                    if posted {
                                        <div class="timeline-time">{update.timestamp.format("%Y-%m-%d %H:%M UTC").to_string()}</div>
                                        <div class={classes!("timeline-card", class)}>
                                            <div class="timeline-card-head">
                                                <span class={classes!("incident-status-pill", class)}>{update.impact.label()}</span>
                                                if is_editing {
                                                    <button type="button" class="icon-button" title="Done editing"
                                                        onclick={ let editing = editing.clone(); Callback::from(move |_| editing.set(None)) }>
                                                        { check_icon() }
                                                    </button>
                                                } else {
                                                    <button type="button" class="icon-button" title="Edit message"
                                                        onclick={ let editing = editing.clone(); Callback::from(move |_| editing.set(Some(i))) }>
                                                        { edit_icon() }
                                                    </button>
                                                }
                                            </div>
                                            if is_editing {
                                                <textarea
                                                    class="timeline-message-input"
                                                    rows="3"
                                                    value={update.message.clone()}
                                                    oninput={on_update_message(i)}
                                                />
                                            } else {
                                                <div class="timeline-card-message markdown">{ render_markdown(&update.message) }</div>
                                            }
                                        </div>
                                    } else {
                                        <div class="timeline-edit-row">
                                            <select onchange={on_update_impact(i)}>
                                                { for [Impact::Offline, Impact::Degraded, Impact::None, Impact::Hidden].into_iter().map(|opt| html! {
                                                    <option value={opt.as_str()} selected={opt == update.impact}>{opt.label()}</option>
                                                }) }
                                            </select>
                                            <span class="timeline-time">{update.timestamp.format("%Y-%m-%d %H:%M UTC").to_string()}</span>
                                            <button type="button" class="link-button danger" onclick={on_remove_update(i)}>{"Remove"}</button>
                                        </div>
                                        <textarea
                                            class="timeline-message-input"
                                            rows="3"
                                            value={update.message.clone()}
                                            oninput={on_update_message(i)}
                                        />
                                    }
                                </div>
                            </li>
                        }
                    }) }
                </ul>

                <div class="incident-admin-controls">
                    <button type="button" onclick={on_add_update}>{"Add update"}</button>
                    <button type="button" class="danger" onclick={on_delete}>{"Delete incident"}</button>
                </div>

                <p class="incident-edit-hint">
                    { format!("Current status: {} · {} update(s)", current.current_impact().label(), current.updates.len()) }
                    <StatusDot class={status_class} size=10 />
                </p>
            </article>
        </div>
    }
}
