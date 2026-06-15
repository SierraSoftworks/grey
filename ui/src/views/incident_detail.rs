use grey_api::Identifier;
use yew::prelude::*;

use crate::components::IncidentBlock;
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
    use crate::components::incidents_timeline::{impact_class, impact_label};
    use crate::routes::Route;
    use crate::views::{impact_value, parse_impact};
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
    let navigator = use_navigator();

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

    let dirty = current.title != *title || current.updates != *updates;

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
                u.impact = parse_impact(&el.value());
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
            saving.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                let result = crate::api::replace_incident(&token, &id, version, &edit).await;
                saving.set(false);
                match result {
                    Ok(incident) => {
                        title.set(incident.title.clone());
                        updates.set(incident.sorted_updates().into_iter().cloned().collect());
                        loaded.set(Some(incident));
                        error.set(None);
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
            });
        })
    };

    let on_delete = {
        let token = props.token.clone();
        let id = current.id.to_string();
        let navigator = navigator.clone();
        let error = error.clone();
        Callback::from(move |_| {
            let token = token.clone();
            let id = id.clone();
            let navigator = navigator.clone();
            let error = error.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match crate::api::delete_incident(&token, &id).await {
                    Ok(()) => {
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
                    <input
                        class="incident-title-input"
                        type="text"
                        value={(*title).clone()}
                        oninput={on_title}
                    />
                    <div class="incident-edit-actions">
                        <span class="incident-id">{format!("#{}", current.id)}</span>
                        if dirty {
                            <button class="save-button" title="Save changes" disabled={*saving} onclick={on_save}>
                                { if *saving { "Saving…" } else { "💾 Save" } }
                            </button>
                        }
                    </div>
                </div>

                if let Some(err) = (*error).clone() {
                    <p class="incidents-error">{err}</p>
                }

                <ul class="incident-timeline editing">
                    { for (*updates).iter().enumerate().map(|(i, update)| {
                        let class = impact_class(update.impact);
                        html! {
                            <li class="timeline-item">
                                <div class="timeline-rail">
                                    <span class={classes!("timeline-circle", class)}></span>
                                    if i + 1 != updates.len() {
                                        <span class={classes!("timeline-tail", class)}></span>
                                    }
                                </div>
                                <div class="timeline-body">
                                    <div class="timeline-edit-row">
                                        <select onchange={on_update_impact(i)}>
                                            { for [Impact::Offline, Impact::Degraded, Impact::None, Impact::Hidden].into_iter().map(|opt| html! {
                                                <option value={impact_value(opt)} selected={opt == update.impact}>{impact_label(opt)}</option>
                                            }) }
                                        </select>
                                        <span class="timeline-time">{update.timestamp.format("%Y-%m-%d %H:%M UTC").to_string()}</span>
                                        <button type="button" class="link-button danger" onclick={on_remove_update(i)}>{"Remove"}</button>
                                    </div>
                                    <textarea
                                        class="timeline-message-input"
                                        rows="2"
                                        value={update.message.clone()}
                                        oninput={on_update_message(i)}
                                    />
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
                    { format!("Current status: {} · {} update(s)", impact_label(current.current_impact()), current.updates.len()) }
                    <span class={classes!("status-dot-inline", status_class)}></span>
                </p>
            </article>
        </div>
    }
}
