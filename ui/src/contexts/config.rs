use grey_api::UiConfig;
use yew::prelude::*;

#[derive(Clone, PartialEq)]
pub struct UiConfigContext {
    pub config: UiConfig,
}

impl Default for UiConfigContext {
    fn default() -> Self {
        Self {
            config: UiConfig::default(),
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct UiConfigProviderProps {
    pub config: UiConfig,
    pub children: Children,
}

#[function_component(UiConfigProvider)]
pub fn ui_config_provider(props: &UiConfigProviderProps) -> Html {
    let context = UiConfigContext {
        config: props.config.clone(),
    };

    html! {
        <ContextProvider<UiConfigContext> context={context}>
            {props.children.clone()}
        </ContextProvider<UiConfigContext>>
    }
}

#[hook]
pub fn use_ui_config() -> UiConfigContext {
    use_context::<UiConfigContext>().expect("UiConfigContext not found. Make sure to wrap your component with UiConfigProvider.")
}
