use grey_api::Peer;
use yew::prelude::*;

#[derive(Clone, PartialEq)]
pub struct PeersContext {
    pub peers: Vec<Peer>,
}

#[derive(Properties, PartialEq)]
pub struct PeersProviderProps {
    pub peers: Vec<Peer>,
    pub children: Children,
}

#[function_component(PeersProvider)]
pub fn peers_provider(props: &PeersProviderProps) -> Html {
    let context = PeersContext {
        peers: props.peers.clone(),
    };

    html! {
        <ContextProvider<PeersContext> context={context}>
            {props.children.clone()}
        </ContextProvider<PeersContext>>
    }
}

#[hook]
pub fn use_peers() -> PeersContext {
    use_context::<PeersContext>()
        .expect("PeersContext not found. Make sure to wrap your component with PeersProvider.")
}
