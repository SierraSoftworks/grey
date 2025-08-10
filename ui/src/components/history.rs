use yew::prelude::*;

use super::probe::SampleData;

#[derive(Properties, PartialEq)]
pub struct HistoryProps {
    pub samples: Vec<SampleData>,
}

#[function_component(History)]
pub fn history(props: &HistoryProps) -> Html {
    html! {
        <div class="history">
            {for props.samples.iter().map(|sample| {
                let sample_class = if sample.pass { "ok" } else { "error" };
                html! {
                    <span 
                        class={format!("history-sample {}", sample_class)}
                        title={sample.message.clone()}
                    ></span>
                }
            })}
        </div>
    }
}
