use yew::prelude::*;
use super::super::client::{ServerApp, AppData};

#[derive(Properties, PartialEq)]
pub struct PageProps {
    pub app_data: AppData,
}

#[function_component(Page)]
pub fn page(props: &PageProps) -> Html {
    html! {
        <html>
            <head>
                <title>{&props.app_data.config.title}</title>
                <meta http-equiv="Content-Type" content="text/html; charset=utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1.0" />
                <link rel="stylesheet" href="/static/grey_wasm.css" />
            </head>
            <body>
                <ServerApp data={props.app_data.clone()} />
                
                // Include the WASM hydration script
                <script type="module">
                    {r#"
                    import init, { hydrate_app } from '/static/grey_wasm.js';
                    
                    async function run() {
                        await init();
                        hydrate_app();
                    }
                    
                    run();
                    "#}
                </script>
            </body>
        </html>
    }
}
