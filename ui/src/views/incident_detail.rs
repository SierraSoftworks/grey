use grey_api::Identifier;
use yew::prelude::*;

use yewi::prelude::*;

use crate::components::IncidentBlock;
use crate::contexts::use_store;
use crate::routes::Route;
// `time_format` is only used by the wasm-only admin editor below, so it is unused in the SSR build.
#[cfg(not(feature = "ssr"))]
use crate::formatters::time_format;

/// The trail back to the incident list, shown above every incident.
fn breadcrumbs(title: &str) -> Html {
    html! {
        <Breadcrumbs>
            <Crumb href={NavTarget::route(Route::Incidents)}>{ "Incidents" }</Crumb>
            <Crumb>{ title }</Crumb>
        </Breadcrumbs>
    }
}

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
        if let Some(incident) = incident {
            <div class="incidents">
                { breadcrumbs(incident.title()) }
                <IncidentBlock incident={incident} />
            </div>
        } else {
            <EmptyState
                title="Incident not found"
                icon={icons::SEARCH_X}
                actions={html! { <Button href={NavTarget::route(Route::Incidents)}>{ "All incidents" }</Button> }}
            >
                {"This incident does not exist or is not publicly visible."}
            </EmptyState>
        }
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
    use crate::components::incident_timeline::message;
    use crate::formatters::{changed_update_timestamp, datetime_local_value, incident_status, resolve_update_timestamp};
    use crate::tones::impact_tone;
    use chrono::{DateTime, Utc};
    use grey_api::{CreateUpdate, Impact, IncidentUpdateId, IncidentView, PutIncident, PutUpdate};
    use std::rc::Rc;
    use web_sys::HtmlInputElement;
    use yew_router::prelude::use_navigator;

    // The canonical incident (with its updates + versions) plus the editable draft fields.
    let loaded = use_state(|| Option::<IncidentView>::None);
    let title = use_state(String::new);
    // The update whose message is open in a textarea (by id), plus that textarea's draft and the
    // draft of its timestamp (as a UTC `datetime-local` value, so a mistimed update can be corrected).
    let editing = use_state(|| Option::<IncidentUpdateId>::None);
    let message_draft = use_state(String::new);
    let timestamp_draft = use_state(String::new);
    // The "add update" form. Its timestamp is blank unless the update is being backdated — to when
    // mitigation actually completed, say — in which case the server records that moment instead of now.
    let new_impact = use_state(|| "offline".to_string());
    let new_message = use_state(String::new);
    let new_timestamp = use_state(String::new);
    let saving = use_state(|| false);
    // Deleting an incident cannot be undone, so it is confirmed first.
    let confirming_delete = use_state(|| false);
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
        return html! { <crate::components::IncidentBlockSkeleton /> };
    };

    let incident_version = current.incident.version;
    let title_dirty = current.title() != *title;

    let on_title = {
        let title = title.clone();
        Callback::from(move |value: String| title.set(value))
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
        let new_timestamp = new_timestamp.clone();
        let saving = saving.clone();
        let store = store.clone();
        let apply_saved = apply_saved.clone();
        Callback::from(move |_| {
            let message = (*new_message).trim().to_string();
            if message.is_empty() {
                store.set_error(grey_api::ApiError::new("An update message is required."));
                return;
            }
            let timestamp = match resolve_update_timestamp(&new_timestamp) {
                Ok(timestamp) => timestamp,
                Err(e) => {
                    store.set_error(e);
                    return;
                }
            };
            let input =
                CreateUpdate { impact: new_impact.parse().unwrap_or_default(), message, timestamp };
            let (id, new_message, new_timestamp, saving, store, apply_saved) = (
                id.clone(),
                new_message.clone(),
                new_timestamp.clone(),
                saving.clone(),
                store.clone(),
                apply_saved.clone(),
            );
            saving.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                match store.create_update(id, input).await {
                    Ok(view) => {
                        apply_saved(view);
                        new_message.set(String::new());
                        new_timestamp.set(String::new());
                    }
                    Err(e) => store.set_error(e),
                }
                saving.set(false);
            });
        })
    };

    let on_save_update = |uid: IncidentUpdateId, version: u64, posted_at: DateTime<Utc>| {
        let message_draft = message_draft.clone();
        let timestamp_draft = timestamp_draft.clone();
        let editing = editing.clone();
        let saving = saving.clone();
        let store = store.clone();
        let apply_saved = apply_saved.clone();
        Callback::from(move |_| {
            // A blank or untouched time leaves the stored timestamp alone; an edited one moves the
            // update along the timeline.
            let timestamp = match changed_update_timestamp(&timestamp_draft, posted_at) {
                Ok(timestamp) => timestamp,
                Err(e) => {
                    store.set_error(e);
                    return;
                }
            };
            let edit = PutUpdate { message: (*message_draft).clone(), timestamp };
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
        let confirming_delete = confirming_delete.clone();
        Callback::from(move |_| {
            let (navigator, store) = (navigator.clone(), store.clone());
            confirming_delete.set(false);
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

    // Newest update first in the editor.
    let mut updates = current.updates.clone();
    updates.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let on_new_impact = {
        let new_impact = new_impact.clone();
        Callback::from(move |value: String| new_impact.set(value))
    };
    let on_new_message = {
        let new_message = new_message.clone();
        Callback::from(move |value: String| new_message.set(value))
    };
    let on_new_timestamp = {
        let new_timestamp = new_timestamp.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlInputElement = e.target_unchecked_into();
            new_timestamp.set(el.value());
        })
    };
    let on_draft_message = {
        let message_draft = message_draft.clone();
        Callback::from(move |value: String| message_draft.set(value))
    };
    let on_draft_timestamp = {
        let timestamp_draft = timestamp_draft.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlInputElement = e.target_unchecked_into();
            timestamp_draft.set(el.value());
        })
    };
    let on_cancel_edit = {
        let editing = editing.clone();
        Callback::from(move |_| editing.set(None))
    };
    let on_ask_delete = {
        let confirming_delete = confirming_delete.clone();
        Callback::from(move |_: MouseEvent| confirming_delete.set(true))
    };
    let on_cancel_delete = {
        let confirming_delete = confirming_delete.clone();
        Callback::from(move |_: ()| confirming_delete.set(false))
    };

    let impact_options: Rc<[(AttrValue, AttrValue)]> =
        Impact::ALL.into_iter().map(|impact| (impact.as_str().into(), impact.label().into())).collect();
    let new_tone = impact_tone(new_impact.parse().unwrap_or_default());
    let (status_text, status_tone) = incident_status(&current);

    html! {
        <div class="incidents incident-edit">
            { breadcrumbs(current.title()) }
            <Card class="incident">
                <div class="incident__header">
                    <div class="incident-edit__title">
                        <TextInput
                            size={Size::Large}
                            value={(*title).clone()}
                            on_input={on_title}
                            attrs={attrs! { "aria-label" => "Incident title" }}
                        />
                        if title_dirty {
                            <IconButton
                                icon={icons::SAVE}
                                label="Save title"
                                variant={ButtonVariant::Primary}
                                loading={*saving}
                                on_click={on_save_title}
                            />
                        }
                    </div>
                    <Pill tone={status_tone} active={current.is_active()}>{ status_text }</Pill>
                </div>

                <Timeline label="Updates" run_off={TimelineRunOff::End}>
                    // The next update: where the incident stands now, and what to tell people.
                    <TimelineItem
                        class="incident-edit__new"
                        tone={new_tone}
                        marker={TimelineMarker::Ring}
                        title={html! { { "Post an update" } }}
                    >
                        <div class="incident-edit__form">
                            <TextArea
                                rows=3
                                value={(*new_message).clone()}
                                on_input={on_new_message}
                                placeholder="Enter your next incident update in markdown form..."
                                attrs={attrs! { "aria-label" => "Update message" }}
                            />
                            <div class="incident-edit__controls">
                                <NativeSelect
                                    label="Impact"
                                    fit=true
                                    options={impact_options}
                                    value={(*new_impact).clone()}
                                    on_change={on_new_impact}
                                />
                                <input
                                    class="yi-input yi-input--md incident-edit__time"
                                    type="datetime-local"
                                    aria-label="When this update happened (UTC)"
                                    title="When this update happened (UTC) — leave blank to post it as of now"
                                    value={(*new_timestamp).clone()}
                                    oninput={on_new_timestamp}
                                />
                                <Button
                                    variant={ButtonVariant::Primary}
                                    icon={icons::SEND}
                                    disabled={*saving}
                                    on_click={on_add_update}
                                >
                                    { "Post update" }
                                </Button>
                            </div>
                        </div>
                    </TimelineItem>

                    { for updates.iter().map(|update| {
                        let uid = update.id;
                        let version = update.version;
                        let is_editing = *editing == Some(uid);
                        let on_edit = {
                            let editing = editing.clone();
                            let message_draft = message_draft.clone();
                            let timestamp_draft = timestamp_draft.clone();
                            let msg = update.message.clone();
                            let at = datetime_local_value(update.timestamp);
                            Callback::from(move |_| {
                                message_draft.set(msg.clone());
                                timestamp_draft.set(at.clone());
                                editing.set(Some(uid));
                            })
                        };
                        let aside = if is_editing {
                            html! {
                                <>
                                    <input
                                        class="yi-input yi-input--sm incident-edit__time"
                                        type="datetime-local"
                                        aria-label="When this update happened (UTC)"
                                        title="When this update happened (UTC)"
                                        value={(*timestamp_draft).clone()}
                                        oninput={on_draft_timestamp.clone()}
                                    />
                                    <IconButton
                                        icon={icons::CHECK}
                                        label="Save update"
                                        size={Size::Small}
                                        variant={ButtonVariant::Primary}
                                        disabled={*saving}
                                        on_click={on_save_update(uid, version, update.timestamp)}
                                    />
                                    <IconButton
                                        icon={icons::X}
                                        label="Cancel"
                                        size={Size::Small}
                                        variant={ButtonVariant::Subtle}
                                        on_click={on_cancel_edit.clone()}
                                    />
                                </>
                            }
                        } else {
                            html! {
                                <>
                                    <time datetime={update.timestamp.to_rfc3339()}>{ time_format(update.timestamp) }</time>
                                    <IconButton
                                        icon={icons::PENCIL}
                                        label="Edit update"
                                        size={Size::Small}
                                        variant={ButtonVariant::Subtle}
                                        on_click={on_edit}
                                    />
                                    <IconButton
                                        icon={icons::TRASH_2}
                                        label="Remove update"
                                        size={Size::Small}
                                        variant={ButtonVariant::Subtle}
                                        disabled={*saving}
                                        on_click={on_remove_update(uid, version)}
                                    />
                                </>
                            }
                        };
                        html! {
                            <TimelineItem
                                key={uid.to_string()}
                                tone={impact_tone(update.impact)}
                                title={html! { { update.impact.label() } }}
                                {aside}
                            >
                                if is_editing {
                                    <TextArea
                                        rows=3
                                        value={(*message_draft).clone()}
                                        on_input={on_draft_message.clone()}
                                        attrs={attrs! { "aria-label" => "Update message" }}
                                    />
                                } else {
                                    { message(update) }
                                }
                            </TimelineItem>
                        }
                    }) }
                </Timeline>
            </Card>

            <div class="incident-edit__footer">
                <Text size={TextSize::Sm} color={TextColor::Muted}>
                    { format!("#{} · {} update(s)", current.id(), current.updates.len()) }
                </Text>
                <Button
                    variant={ButtonVariant::Danger}
                    icon={icons::TRASH_2}
                    disabled={*saving}
                    on_click={on_ask_delete}
                >
                    { "Delete incident" }
                </Button>
            </div>

            <ConfirmDialog
                open={*confirming_delete}
                title={format!("Delete \u{201c}{}\u{201d}?", current.title())}
                confirm_label="Delete incident"
                danger=true
                on_confirm={on_delete}
                on_cancel={on_cancel_delete}
            >
                { "The incident and every update posted to it are removed from the status page. This cannot be undone." }
            </ConfirmDialog>
        </div>
    }
}
