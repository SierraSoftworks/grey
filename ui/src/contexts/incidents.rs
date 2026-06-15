use grey_api::{Identifier, Incident};
use yew::prelude::*;

#[derive(Clone, PartialEq)]
pub struct IncidentsContext {
    pub incidents: Vec<Incident>,
    /// Insert or replace an incident in the in-memory list. Admin create/edit flows call this with the
    /// server's authoritative response so every view (landing page, lists, detail) reflects the change
    /// immediately, instead of waiting for the next poll.
    pub upsert: Callback<Incident>,
    /// Remove an incident from the in-memory list (after an admin delete).
    pub remove: Callback<Identifier>,
}

#[derive(Properties, PartialEq)]
pub struct IncidentsProviderProps {
    pub incidents: Vec<Incident>,
    pub upsert: Callback<Incident>,
    pub remove: Callback<Identifier>,
    pub children: Children,
}

#[function_component(IncidentsProvider)]
pub fn incidents_provider(props: &IncidentsProviderProps) -> Html {
    let context = IncidentsContext {
        incidents: props.incidents.clone(),
        upsert: props.upsert.clone(),
        remove: props.remove.clone(),
    };

    html! {
        <ContextProvider<IncidentsContext> context={context}>
            {props.children.clone()}
        </ContextProvider<IncidentsContext>>
    }
}

#[hook]
pub fn use_incidents() -> IncidentsContext {
    use_context::<IncidentsContext>().expect(
        "IncidentsContext not found. Make sure to wrap your component with IncidentsProvider.",
    )
}
