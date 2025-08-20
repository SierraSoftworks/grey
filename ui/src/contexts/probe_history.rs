use grey_api::ProbeHistory;
use std::collections::HashMap;
use yew::prelude::*;

#[derive(Clone, PartialEq)]
pub struct ProbeHistoryContext {
    pub probe_histories: HashMap<String, Vec<ProbeHistory>>,
}

#[derive(Properties, PartialEq)]
pub struct ProbeHistoryProviderProps {
    pub probe_histories: HashMap<String, Vec<ProbeHistory>>,
    pub children: Children,
}

#[function_component(ProbeHistoryProvider)]
pub fn probe_history_provider(props: &ProbeHistoryProviderProps) -> Html {
    let context = ProbeHistoryContext {
        probe_histories: props.probe_histories.clone(),
    };

    html! {
        <ContextProvider<ProbeHistoryContext> context={context}>
            {props.children.clone()}
        </ContextProvider<ProbeHistoryContext>>
    }
}

#[hook]
pub fn use_probe_history() -> ProbeHistoryContext {
    use_context::<ProbeHistoryContext>().expect("ProbeHistoryContext not found. Make sure to wrap your component with ProbeHistoryProvider.")
}
