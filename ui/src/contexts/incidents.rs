use grey_api::Incident;
use yew::prelude::*;

#[derive(Clone, PartialEq)]
pub struct IncidentsContext {
    pub incidents: Vec<Incident>,
}

#[derive(Properties, PartialEq)]
pub struct IncidentsProviderProps {
    pub incidents: Vec<Incident>,
    pub children: Children,
}

#[function_component(IncidentsProvider)]
pub fn incidents_provider(props: &IncidentsProviderProps) -> Html {
    let context = IncidentsContext {
        incidents: props.incidents.clone(),
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
