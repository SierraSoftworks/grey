//! The pieces shared by every history popover (a probe's availability bucket, a cron's run): a
//! status line for the head, and `label: value` rows for the body.

use yew::prelude::*;
use yewi::prelude::*;

/// The head of a history popover: a status dot and a short label.
pub fn status_line(tone: Tone, label: impl Into<AttrValue>) -> Html {
    let label: AttrValue = label.into();
    html! {
        <span class="detail__status">
            <StatusDot {tone} />
            <span>{ label }</span>
        </span>
    }
}

/// One `label: value` row of a history popover.
pub fn row(label: &'static str, value: impl Into<Html>) -> Html {
    html! {
        <div class="detail__row">
            <span class="detail__label">{ label }</span>
            <span class="detail__value">{ value.into() }</span>
        </div>
    }
}

/// A titled group of entries under the rows, such as the checks a probe ran.
pub fn section(title: &'static str, entries: Html) -> Html {
    html! {
        <div class="detail__section">
            <div class="detail__section-title">{ title }</div>
            { entries }
        </div>
    }
}

/// One entry of a [`section`]: a pass/fail dot, a name and an optional note beside or beneath it.
pub fn entry(tone: Tone, name: impl Into<Html>, note: Option<Html>) -> Html {
    html! {
        <div class="detail__entry">
            <StatusDot {tone} />
            <span class="detail__entry-name">{ name.into() }</span>
            if let Some(note) = note {
                <span class="detail__entry-note">{ note }</span>
            }
        </div>
    }
}
