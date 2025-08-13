use grey_api::UiNotice;
use yew::prelude::*;

#[derive(Clone, PartialEq)]
pub struct NoticesContext {
    pub notices: Vec<UiNotice>,
}

#[derive(Properties, PartialEq)]
pub struct NoticesProviderProps {
    pub notices: Vec<UiNotice>,
    pub children: Children,
}

#[function_component(NoticesProvider)]
pub fn notices_provider(props: &NoticesProviderProps) -> Html {
    let context = NoticesContext {
        notices: props.notices.clone(),
    };

    html! {
        <ContextProvider<NoticesContext> context={context}>
            {props.children.clone()}
        </ContextProvider<NoticesContext>>
    }
}

#[hook]
pub fn use_notices() -> NoticesContext {
    use_context::<NoticesContext>()
        .expect("NoticesContext not found. Make sure to wrap your component with NoticesProvider.")
}
