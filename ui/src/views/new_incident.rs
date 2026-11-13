use yew::prelude::*;
use yewi::prelude::*;

use crate::contexts::use_store;

/// The `/incidents/new` page (admin only): a title plus the incident's opening update (impact,
/// message, and optionally the time it started, so an outage can be declared after the fact). Saving
/// creates the incident and navigates to its page.
#[function_component(NewIncident)]
pub fn new_incident() -> Html {
    let store = use_store();

    if !store.is_authenticated() {
        return html! {
            <EmptyState title="Sign in to declare an incident" icon={icons::LOCK}>
                {"Sign in as an administrator to create an incident."}
            </EmptyState>
        };
    }

    #[cfg(feature = "wasm")]
    if let Some(token) = store.token() {
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
    use crate::formatters::resolve_update_timestamp;
    use crate::routes::Route;
    use grey_api::Impact;
    use std::rc::Rc;
    use web_sys::HtmlInputElement;
    use yew_router::prelude::use_navigator;

    // The `<select>` captions are richer than `Impact::label()` ("Operational (no impact)" rather
    // than "Operational"), so the order/value list comes from `Impact::ALL` while the wording stays
    // here.
    fn impact_caption(impact: Impact) -> &'static str {
        match impact {
            Impact::Offline => "Offline",
            Impact::Degraded => "Degraded",
            Impact::None => "Operational (no impact)",
            Impact::Hidden => "Hidden (draft)",
        }
    }

    let title = use_state(String::new);
    let impact = use_state(|| "offline".to_string());
    let message = use_state(String::new);
    // When the incident started, as a UTC `datetime-local` value. Blank means "now" — the server
    // stamps the opening update itself.
    let started = use_state(String::new);
    let saving = use_state(|| false);
    let navigator = use_navigator();
    // The shared store, whose `create_incident` performs the API call and reflects the new incident
    // in the in-memory list at once.
    let store = use_store();

    let on_title = {
        let title = title.clone();
        Callback::from(move |value: String| title.set(value))
    };
    let on_impact = {
        let impact = impact.clone();
        Callback::from(move |value: String| impact.set(value))
    };
    let on_started = {
        let started = started.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlInputElement = e.target_unchecked_into();
            started.set(el.value());
        })
    };
    let on_message = {
        let message = message.clone();
        Callback::from(move |value: String| message.set(value))
    };

    let onsubmit = {
        let token = props.token.clone();
        let (title, impact, message, started) =
            (title.clone(), impact.clone(), message.clone(), started.clone());
        let saving = saving.clone();
        let navigator = navigator.clone();
        let store = store.clone();
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let title_value = (*title).trim().to_string();
            let message_value = (*message).trim().to_string();
            if title_value.is_empty() || message_value.is_empty() {
                store.set_error(grey_api::ApiError::new(
                    "A title and an initial update message are required.",
                ));
                return;
            }
            // A blank start time leaves the stamping to the server; anything else backdates the
            // opening update to when the problem actually began.
            let timestamp = match resolve_update_timestamp(&started) {
                Ok(timestamp) => timestamp,
                Err(e) => {
                    store.set_error(e);
                    return;
                }
            };
            let input = grey_api::CreateIncident {
                title: title_value,
                impact: impact.parse().unwrap_or_default(),
                message: message_value,
                timestamp,
            };
            let _ = &token;
            let saving = saving.clone();
            let navigator = navigator.clone();
            let store = store.clone();
            saving.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                match store.create_incident(input).await {
                    Ok(created) => {
                        if let Some(nav) = navigator {
                            nav.push(&Route::Incident { id: created.id().to_string() });
                        }
                    }
                    Err(e) => {
                        saving.set(false);
                        store.set_error(e);
                    }
                }
            });
        })
    };

    let impact_options: Rc<[(AttrValue, AttrValue)]> =
        Impact::ALL.into_iter().map(|opt| (opt.as_str().into(), impact_caption(opt).into())).collect();

    html! {
        <>
            <PageHeader
                title="Declare an incident"
                subtitle={html! { { "Tell people what is affected. You can post further updates as the incident is worked on." } }}
                breadcrumbs={html! {
                    <Breadcrumbs>
                        <Crumb href={NavTarget::route(Route::Incidents)}>{ "Incidents" }</Crumb>
                        <Crumb>{ "New" }</Crumb>
                    </Breadcrumbs>
                }}
            />
            <div class="incident-form">
                <form onsubmit={onsubmit}>
                    <Stack gap={Space::Lg}>
                        <Field label={html! { { "Title" } }}>
                            <TextInput value={(*title).clone()} on_input={on_title} />
                        </Field>
                        <div class="incident-form__row">
                            <Field label={html! { { "Initial impact" } }}>
                                <NativeSelect options={impact_options} value={(*impact).clone()} on_change={on_impact} />
                            </Field>
                            <Field
                                label={html! { { "Started (UTC)" } }}
                                hint={html! { { "Leave blank to start the incident now, or backdate it to when the problem began." } }}
                            >
                                <StartedInput value={(*started).clone()} oninput={on_started} />
                            </Field>
                        </div>
                        <Field label={html! { { "Initial update" } }}>
                            <TextArea
                                rows=4
                                value={(*message).clone()}
                                on_input={on_message}
                                placeholder="Enter initial update in markdown form..."
                            />
                        </Field>
                        <div class="incident-form__actions">
                            <Button href={NavTarget::route(Route::Incidents)} variant={ButtonVariant::Subtle}>{ "Cancel" }</Button>
                            <Button kind={ButtonKind::Submit} variant={ButtonVariant::Primary} loading={*saving}>
                                { "Create incident" }
                            </Button>
                        </div>
                    </Stack>
                </form>
            </div>
        </>
    }
}

/// The start time as a native date and time picker, wired to its [`Field`] (label, hint).
#[cfg(feature = "wasm")]
#[derive(Properties, PartialEq)]
struct StartedInputProps {
    value: AttrValue,
    oninput: Callback<InputEvent>,
}

#[cfg(feature = "wasm")]
#[function_component(StartedInput)]
fn started_input(props: &StartedInputProps) -> Html {
    let field = use_field_control(None, false, false, false);
    html! {
        <input
            class="yi-input yi-input--md"
            type="datetime-local"
            id={field.id}
            aria-describedby={field.described_by}
            value={props.value.clone()}
            oninput={props.oninput.clone()}
        />
    }
}

#[cfg(all(test, feature = "wasm"))]
mod tests {
    use super::*;
    use crate::contexts::StoreProvider;

    /// Server-renders the admin form the way a signed-in operator sees it. Effects never run during
    /// SSR, so this exercises the markup and the hooks that build it, not the submit path.
    async fn render_form() -> String {
        #[function_component(Harness)]
        fn harness() -> Html {
            html! {
                <StoreProvider>
                    <NewIncidentForm token={"test-token".to_string()} />
                </StoreProvider>
            }
        }

        yew::ServerRenderer::<Harness>::new().render().await
    }

    #[tokio::test]
    async fn offers_a_start_time_alongside_the_opening_update() {
        let html = render_form().await;
        assert!(html.contains("Started (UTC)"), "the start time is labelled as UTC: {html}");
        assert!(html.contains("type=\"datetime-local\""), "it is entered with a date/time picker");
        assert!(html.contains("Leave blank to start the incident now"), "blank means now");
        assert!(html.contains("Create incident"));
    }
}
