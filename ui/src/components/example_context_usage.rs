use yew::prelude::*;
use crate::contexts::{use_ui_config, use_probes, use_notices, use_probe_history};

/// Example component demonstrating how to use the context providers
#[function_component(ExampleContextUsage)]
pub fn example_context_usage() -> Html {
    let config_ctx = use_ui_config();
    let probes_ctx = use_probes();
    let notices_ctx = use_notices();
    let history_ctx = use_probe_history();

    html! {
        <div style="padding: 1rem; border: 1px solid #ccc; margin: 1rem; background: #f9f9f9;">
            <h3>{"Context Usage Example"}</h3>
            
            <div style="margin: 0.5rem 0;">
                <strong>{"UI Title: "}</strong>
                {&config_ctx.config.title}
            </div>
            
            <div style="margin: 0.5rem 0;">
                <strong>{"Number of Probes: "}</strong>
                {probes_ctx.probes.len()}
            </div>
            
            <div style="margin: 0.5rem 0;">
                <strong>{"Number of Notices: "}</strong>
                {notices_ctx.notices.len()}
            </div>
            
            <div style="margin: 0.5rem 0;">
                <strong>{"Probes with History: "}</strong>
                {history_ctx.probe_histories.len()}
            </div>
            
            <details style="margin-top: 1rem;">
                <summary>{"Probe Names"}</summary>
                <ul>
                    {for probes_ctx.probes.iter().map(|probe| {
                        html! {
                            <li>{&probe.name}</li>
                        }
                    })}
                </ul>
            </details>
        </div>
    }
}
