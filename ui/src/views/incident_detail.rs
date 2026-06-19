use grey_api::Identifier;
use yew::prelude::*;

use crate::components::IncidentBlock;
use crate::contexts::use_store;
// `StatusDot` and `time_format` are only used by the wasm-only admin editor below, so they are
// unused in the SSR build.
#[cfg(not(feature = "ssr"))]
use crate::components::StatusDot;
#[cfg(not(feature = "ssr"))]
use crate::formatters::time_format;

/// The `/incidents/{id}` page. Public visitors see the read-only incident; signed-in administrators
/// get an inline editor: edit the title, add updates, edit an update's message, remove updates, and
/// delete the incident. Each action is its own check-and-set API call against that entity's version.
#[derive(Properties, PartialEq)]
pub struct IncidentDetailProps {
    pub id: String,
}

#[function_component(IncidentDetail)]
pub fn incident_detail(props: &IncidentDetailProps) -> Html {
    let store = use_store();

    #[cfg(feature = "wasm")]
    if store.is_authenticated() {
        if let Some(token) = store.token() {
            return html! { <AdminIncidentDetail id={props.id.clone()} token={token} /> };
        }
    }
    #[cfg(not(feature = "wasm"))]
    let _ = &store;

    let wanted = Identifier::parse(&props.id);
    let incident = store.incidents().iter().find(|i| Some(i.id()) == wanted).cloned();

    html! {
        <div class="page">
            if let Some(incident) = incident {
                <IncidentBlock incident={incident} />
            } else {
                <crate::components::EmptyState title="Incident not found">
                    {"This incident does not exist or is not publicly visible."}
                </crate::components::EmptyState>
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
    use crate::components::icons::{check_icon, edit_icon, save_icon, trash_icon};
    use crate::components::markdown::render_markdown;
    use crate::routes::Route;
    use crate::styles::impact_class;
    use grey_api::{CreateUpdate, Impact, IncidentUpdateId, IncidentView, PutIncident, PutUpdate};
    use web_sys::{HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement};
    use yew_router::prelude::*;

    // The canonical incident (with its updates + versions) plus the editable draft fields.
    let loaded = use_state(|| Option::<IncidentView>::None);
    let title = use_state(String::new);
    // The update whose message is open in a textarea (by id), plus that textarea's draft.
    let editing = use_state(|| Option::<IncidentUpdateId>::None);
    let message_draft = use_state(String::new);
    // The "add update" form.
    let new_impact = use_state(|| "offline".to_string());
    let new_message = use_state(String::new);
    let saving = use_state(|| false);
    let navigator = use_navigator();
    let store = use_store();
    let client = store.client().clone();

    // Load the incident (including hidden) once.
    {
        let id = props.id.clone();
        let new_impact = new_impact.clone();
        let loaded = loaded.clone();
        let title = title.clone();
        let store = store.clone();
        let client = client.clone();
        use_effect_with((props.token.clone(), props.id.clone()), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                match client.incident(&id).await {
                    Ok(incident) => {
                        title.set(incident.title().to_string());
                        new_impact.set(incident.current_impact().as_str().to_string());
                        loaded.set(Some(incident));
                    }
                    Err(e) => store.set_error(e),
                }
            });
            || ()
        });
    }

    let Some(current) = (*loaded).clone() else {
        return html! {
            <div class="page">
                <crate::components::IncidentBlockSkeleton />
            </div>
        };
    };

    let incident_version = current.incident.version;
    let title_dirty = current.title() != *title;

    let on_title = {
        let title = title.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlInputElement = e.target_unchecked_into();
            title.set(el.value());
        })
    };

    // Replaces the just-saved incident everywhere and refreshes the local draft state.
    let apply_saved = {
        let loaded = loaded.clone();
        let title = title.clone();
        move |view: IncidentView| {
            title.set(view.title().to_string());
            loaded.set(Some(view));
        }
    };

    let on_save_title = {
        let id = current.id().to_string();
        let title = title.clone();
        let saving = saving.clone();
        let store = store.clone();
        let apply_saved = apply_saved.clone();
        Callback::from(move |_| {
            let edit = PutIncident { title: (*title).trim().to_string() };
            let (id, saving, store, apply_saved) = (id.clone(), saving.clone(), store.clone(), apply_saved.clone());
            saving.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                match store.put_incident(id, incident_version, edit).await {
                    Ok(view) => apply_saved(view),
                    Err(e) => store.set_error(e),
                }
                saving.set(false);
            });
        })
    };

    let on_add_update = {
        let id = current.id().to_string();
        let new_impact = new_impact.clone();
        let new_message = new_message.clone();
        let saving = saving.clone();
        let store = store.clone();
        let apply_saved = apply_saved.clone();
        Callback::from(move |_| {
            let message = (*new_message).trim().to_string();
            if message.is_empty() {
                store.set_error(grey_api::ApiError::new("An update message is required."));
                return;
            }
            let input = CreateUpdate { impact: new_impact.parse().unwrap_or_default(), message };
            let (id, new_message, saving, store, apply_saved) =
                (id.clone(), new_message.clone(), saving.clone(), store.clone(), apply_saved.clone());
            saving.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                match store.create_update(id, input).await {
                    Ok(view) => { apply_saved(view); new_message.set(String::new()); }
                    Err(e) => store.set_error(e),
                }
                saving.set(false);
            });
        })
    };

    let on_save_update = |uid: IncidentUpdateId, version: u64| {
        let message_draft = message_draft.clone();
        let editing = editing.clone();
        let saving = saving.clone();
        let store = store.clone();
        let apply_saved = apply_saved.clone();
        Callback::from(move |_| {
            let edit = PutUpdate { message: (*message_draft).clone() };
            let (editing, saving, store, apply_saved) =
                (editing.clone(), saving.clone(), store.clone(), apply_saved.clone());
            saving.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                match store.put_update(uid, version, edit).await {
                    Ok(view) => { apply_saved(view); editing.set(None); }
                    Err(e) => store.set_error(e),
                }
                saving.set(false);
            });
        })
    };

    let on_remove_update = |uid: IncidentUpdateId, version: u64| {
        let saving = saving.clone();
        let store = store.clone();
        Callback::from(move |_| {
            let (saving, store) = (saving.clone(), store.clone());
            saving.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                if let Err(e) = store.delete_update(uid, version).await {
                    store.set_error(e);
                }
                saving.set(false);
            });
        })
    };

    let on_delete = {
        let id_value = current.id();
        let navigator = navigator.clone();
        let store = store.clone();
        Callback::from(move |_| {
            let (navigator, store) = (navigator.clone(), store.clone());
            wasm_bindgen_futures::spawn_local(async move {
                match store.delete_incident(id_value, incident_version).await {
                    Ok(()) => {
                        if let Some(nav) = navigator {
                            nav.push(&Route::Incidents);
                        }
                    }
                    Err(e) => store.set_error(e),
                }
            });
        })
    };

    let status_class = impact_class(current.current_impact());
    // Newest update first in the editor.
    let mut updates = current.updates.clone();
    updates.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let on_new_impact = {
        let new_impact = new_impact.clone();
        Callback::from(move |e: Event| {
            let el: HtmlSelectElement = e.target_unchecked_into();
            new_impact.set(el.value());
        })
    };
    let on_new_message = {
        let new_message = new_message.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlTextAreaElement = e.target_unchecked_into();
            new_message.set(el.value());
        })
    };
    let on_draft_message = {
        let message_draft = message_draft.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlTextAreaElement = e.target_unchecked_into();
            message_draft.set(el.value());
        })
    };

    html! {
        <div class="page incident-edit">
            <article class="incident-block">
                <div class="incident-block__header">
                    <div class="incident-edit__title-block">
                        <input
                            class="incident-edit__title-input"
                            type="text"
                            value={(*title).clone()}
                            oninput={on_title}
                        />
                        <span class="incident-id">{format!("#{}", current.id())}</span>
                    </div>
                    if title_dirty {
                        <button
                            class={classes!("incident-edit__save-icon", (*saving).then_some("saving"))}
                            title="Save title"
                            disabled={*saving}
                            onclick={on_save_title}
                        >
                            { save_icon() }
                        </button>
                    }
                </div>

                <ul class="incident-timeline incident-timeline--editing">
                    <li class="incident-timeline__item incident-timeline__item--new">
                        <div class="incident-timeline__rail">
                            <span class={classes!("incident-timeline__circle", impact_class(new_impact.parse().unwrap_or_default()))}></span>
                            <span class={classes!("incident-timeline__tail", impact_class(new_impact.parse().unwrap_or_default()))}></span>
                        </div>
                        <div class="incident-timeline__body">
                            <div class="incident-timeline__time">
                                <select onchange={on_new_impact}>
                                    { for Impact::ALL.into_iter().map(|opt| html! {
                                        <option value={opt.as_str()} selected={opt.as_str() == *new_impact}>{opt.label()}</option>
                                    }) }
                                </select>

                                <button type="button" class="incident-edit__icon-button" disabled={*saving} onclick={on_add_update}>{check_icon()}</button>
                            </div>
                            <div class={classes!("incident-timeline__card", status_class)}>
                                <textarea
                                    class="incident-timeline__message-input"
                                    rows="3"
                                    value={(*new_message).clone()}
                                    oninput={on_new_message}
                                    placeholder="Enter your next incident update in markdown form..."
                                />
                            </div>
                        </div>
                    </li>

                    { for updates.iter().map(|update| {
                        let class = impact_class(update.impact);
                        let uid = update.id;
                        let version = update.version;
                        let is_editing = *editing == Some(uid);
                        html! {
                            <li class="incident-timeline__item">
                                <div class="incident-timeline__rail">
                                    <span class={classes!("incident-timeline__circle", class)}></span>
                                    <span class={classes!("incident-timeline__tail", class)}></span>
                                </div>
                                <div class="incident-timeline__body">
                                    <div class="incident-timeline__time">
                                        <span class={classes!("incident-status-pill", class)}>{update.impact.label()}</span>
                                        <time datetime={update.timestamp.to_rfc3339()}>{time_format(update.timestamp)}</time>
                                        if is_editing {
                                            <button type="button" class="incident-edit__icon-button" title="Save message"
                                                disabled={*saving} onclick={on_save_update(uid, version)}>
                                                { check_icon() }
                                            </button>
                                        } else {
                                            <button type="button" class="incident-edit__icon-button" title="Edit message"
                                                onclick={ let editing = editing.clone(); let message_draft = message_draft.clone(); let msg = update.message.clone(); Callback::from(move |_| { message_draft.set(msg.clone()); editing.set(Some(uid)); }) }>
                                                { edit_icon() }
                                            </button>
                                            <button type="button" class="incident-edit__icon-button danger" title="Remove update" disabled={*saving} onclick={on_remove_update(uid, version)}>{ trash_icon() }</button>
                                        }
                                    </div>
                                    <div class={classes!("incident-timeline__card", class)}>
                                        if is_editing {
                                            <textarea
                                                class="incident-timeline__message-input"
                                                rows="3"
                                                value={(*message_draft).clone()}
                                                oninput={on_draft_message.clone()}
                                            />
                                        } else {
                                            <div class="incident-timeline__card-message markdown">{ render_markdown(&update.message) }</div>
                                        }
                                    </div>
                                </div>
                            </li>
                        }
                    }) }
                </ul>

                <div class="incident-admin-controls">
                    <button type="button" class="danger" disabled={*saving} onclick={on_delete}>{"Delete incident"}</button>
                </div>

                <p class="incident-edit__hint">
                    { format!("Current status: {} · {} update(s)", current.current_impact().label(), current.updates.len()) }
                    <StatusDot class={status_class} size=10 />
                </p>
            </article>
        </div>
    }
}
