//! The chrome shared by every page: the app bar (brand, filter, session), the sidebar (pages,
//! services, configured links), the error alert and the footer.

use chrono::Datelike;
use yew::prelude::*;
use yew_router::prelude::use_route;
use yewi::prelude::*;

use super::service_list::{service_anchor, services};
use super::{ClusterStatus, Filter};
use crate::contexts::use_store;
use crate::routes::Route;

#[derive(Properties, PartialEq)]
pub struct ShellProps {
    /// The routed page.
    #[prop_or_default]
    pub children: Html,
}

#[function_component(Shell)]
pub fn shell(props: &ShellProps) -> Html {
    let store = use_store();
    let route = use_route::<Route>().unwrap_or(Route::Home);

    // The filter lives here, above both the app bar (which owns the input) and the routed page
    // (which narrows its list by it), and is cleared when the page changes.
    let query = use_state(String::new);
    let filter = use_memo((*query).clone(), |query| Filter::new(query));
    let on_filter = {
        let query = query.clone();
        Callback::from(move |value: String| query.set(value))
    };
    {
        let query = query.clone();
        use_effect_with(route.clone(), move |_| query.set(String::new()));
    }
    let filter_placeholder = match route {
        Route::Home => Some("Filter by name, service or tag…"),
        Route::Incidents => Some("Filter incidents…"),
        _ => None,
    };

    let brand = html! {
        <AppBarBrand
            logo={html! { <img src={store.config().logo.clone()} alt="" /> }}
            href={NavTarget::route(Route::Home)}
        >
            { &store.config().title }
        </AppBarBrand>
    };

    let actions = html! {
        <>
            if store.is_authenticated() {
                <ClusterStatus />
            }
            <ThemeToggle />
            <Session />
        </>
    };

    let bar = html! {
        <AppBar {brand} {actions}>
            if let Some(placeholder) = filter_placeholder {
                <TextInput
                    class="shell__filter"
                    kind={TextInputKind::Search}
                    value={(*query).clone()}
                    on_input={on_filter}
                    {placeholder}
                    attrs={attrs! { "aria-label" => "Filter" }}
                />
            }
        </AppBar>
    };

    html! {
        <ContextProvider<Filter> context={(*filter).clone()}>
            <AppShell {bar} sidebar={html! { <Navigation /> }} storage_key="grey-sidebar" class="shell">
                <div class="page">
                    <ErrorAlert />
                    { props.children.clone() }
                </div>
                <footer class="shell__footer">
                    <Text tag={TextTag::P} size={TextSize::Sm} color={TextColor::Subtle}>
                        { format!("Copyright © {} Sierra Softworks", chrono::Utc::now().year()) }
                    </Text>
                </footer>
            </AppShell>
        </ContextProvider<Filter>>
    }
}

/// The session at the end of the app bar: the signed-in administrator (with the way out), or a
/// sign-in button when admin authentication is configured.
#[function_component(Session)]
fn session() -> Html {
    let store = use_store();

    if let Some(user) = store.user() {
        // Prefer the user's name, falling back to their email address.
        let name = user.name.clone().or_else(|| user.email.clone()).unwrap_or_else(|| "Admin".to_string());
        let detail = user.name.as_ref().and(user.email.clone());
        return html! {
            <UserMenu
                {name}
                detail={detail.map(AttrValue::from)}
                show_name=true
                // The logout route clears the session and returns home.
                sign_out_href={NavTarget::route(Route::AuthLogout)}
            />
        };
    }

    if store.auth_configured() {
        return html! {
            <Button on_click={store.login.reform(|_| ())}>{ "Sign in" }</Button>
        };
    }

    html! {}
}

/// The sidebar: the pages, the services on the status page (each with its health, linking to its
/// card) and the links the operator configured.
#[function_component(Navigation)]
fn navigation() -> Html {
    let store = use_store();
    let route = use_route::<Route>().unwrap_or(Route::Home);
    let now = chrono::Utc::now();

    let on_status = route == Route::Home;
    let on_incidents = matches!(route, Route::Incidents | Route::NewIncident | Route::Incident { .. });
    let active_incidents = store.incidents().iter().filter(|incident| incident.is_active()).count() as u32;

    html! {
        <Sidebar label="Pages">
            <SidebarItem href={NavTarget::route(Route::Home)} icon={icons::ACTIVITY} current={on_status}>
                { "Status" }
            </SidebarItem>
            <SidebarItem
                href={NavTarget::route(Route::Incidents)}
                icon={icons::TRIANGLE_ALERT}
                current={on_incidents}
                trailing={html! {
                    if active_incidents > 0 {
                        <Badge count={active_incidents} tone={Tone::Warn} label="active incidents" />
                    }
                }}
            >
                { "Incidents" }
            </SidebarItem>
            { controls_item() }

            if !store.probes().is_empty() || !store.crons().is_empty() {
                <SidebarGroup label="Services">
                    { for services(store.probes(), store.crons()).into_iter().map(|(name, group)| {
                        let (tone, summary) = group.summary(now);
                        // From another page the link has to load the status page first.
                        let anchor = service_anchor(name);
                        let href = if on_status { format!("#{anchor}") } else { format!("/#{anchor}") };
                        html! {
                            <SidebarItem
                                key={name.to_string()}
                                {href}
                                nested=true
                                current=false
                                trailing={html! { <StatusDot {tone} label={summary} /> }}
                            >
                                { name }
                            </SidebarItem>
                        }
                    }) }
                </SidebarGroup>
            }

            if !store.config().links.is_empty() {
                <SidebarGroup label="Links">
                    { for store.config().links.iter().map(|link| html! {
                        <SidebarItem
                            key={link.url.clone()}
                            href={NavTarget::href(link.url.clone()).external()}
                            icon={icons::EXTERNAL_LINK}
                        >
                            { &link.title }
                        </SidebarItem>
                    }) }
                </SidebarGroup>
            }
        </Sidebar>
    }
}

/// The debug-only link to the control gallery; absent from release builds, where the route does not
/// exist.
#[cfg(debug_assertions)]
fn controls_item() -> Html {
    html! {
        <SidebarItem href={NavTarget::route(Route::Controls)} icon={icons::COMPONENT}>{ "Controls" }</SidebarItem>
    }
}

#[cfg(not(debug_assertions))]
fn controls_item() -> Html {
    html! {}
}

/// A dismissible alert at the top of the page whenever the store is holding an error (for example
/// a failed background refresh). It shows the error's message, any advice the agent attached for
/// resolving it, and a close button that clears the error from the store.
#[function_component(ErrorAlert)]
pub fn error_alert() -> Html {
    let store = use_store();

    let Some(error) = store.error().cloned() else {
        return html! {};
    };

    html! {
        <Alert
            class="shell__error"
            tone={Tone::Danger}
            title={error.message.clone()}
            on_dismiss={store.clear_error.clone()}
            dismiss_label="Dismiss error"
        >
            if !error.advice.is_empty() {
                <ul>
                    { for error.advice.iter().map(|advice| html! { <li>{ advice }</li> }) }
                </ul>
            }
        </Alert>
    }
}
