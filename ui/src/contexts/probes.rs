use grey_api::Probe;
use yew::prelude::*;

#[derive(Clone, PartialEq)]
pub struct ProbesContext {
    pub probes: Vec<Probe>,
}

#[derive(Properties, PartialEq)]
pub struct ProbesProviderProps {
    pub probes: Vec<Probe>,
    pub children: Children,
}

#[function_component(ProbesProvider)]
pub fn probes_provider(props: &ProbesProviderProps) -> Html {
    let context = ProbesContext {
        probes: props.probes.clone(),
    };

    html! {
        <ContextProvider<ProbesContext> context={context}>
            {props.children.clone()}
        </ContextProvider<ProbesContext>>
    }
}

#[hook]
pub fn use_probes() -> ProbesContext {
    use_context::<ProbesContext>()
        .expect("ProbesContext not found. Make sure to wrap your component with ProbesProvider.")
}
