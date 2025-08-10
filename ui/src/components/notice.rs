use yew::prelude::*;
use grey_api::UiNotice;

#[derive(Properties, PartialEq)]
pub struct NoticeProps {
    pub notice: UiNotice,
}

#[function_component(Notice)]
pub fn notice(props: &NoticeProps) -> Html {
    html! {
        <div class="section notice">
            <h3>{&props.notice.title}</h3>
            <p>{&props.notice.description}</p>
        </div>
    }
}
