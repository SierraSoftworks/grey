use yew::prelude::*;
use crate::contexts::use_notices;
use super::Notice;

#[function_component(Timeline)]
pub fn timeline() -> Html {
    let notices_ctx = use_notices();

    if notices_ctx.notices.is_empty() {
        return html! {};
    }

    html! {
        <div class="notices-timeline">
            <div class="timeline-line"></div>
            {for notices_ctx.notices.iter().map(|notice| {
                html! {
                    <Notice notice={notice.clone()} />
                }
            })}
        </div>
    }
}
