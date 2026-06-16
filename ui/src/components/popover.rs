use yew::prelude::*;

use super::StatusDot;

/// Where a popover anchors over its trigger. Interior triggers centre the popover; triggers at the
/// edges of a row anchor it inward so it never runs off the page. The arrow always points at the
/// trigger.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum PopoverAlign {
    Left,
    #[default]
    Center,
    Right,
}

impl PopoverAlign {
    fn class_name(self) -> &'static str {
        match self {
            PopoverAlign::Left => "popover--align-left",
            PopoverAlign::Center => "popover--align-center",
            PopoverAlign::Right => "popover--align-right",
        }
    }
}

/// A hover popover styled like the probe-history tooltip: a white card with an arrow pointing down
/// to its trigger, a status dot and short label at the top, the caller's content in the middle, and
/// an optional timestamp at the foot.
///
/// The popover positions itself absolutely, so the trigger must be a positioned ancestor (e.g.
/// `position: relative`). The caller controls when it is mounted (typically on hover).
#[derive(Properties, PartialEq)]
pub struct PopoverProps {
    /// How the popover anchors over its trigger.
    #[prop_or_default]
    pub align: PopoverAlign,
    /// Colour class for the status dot (`ok`/`warn`/`error`/`unknown`/`draft`).
    pub status_class: AttrValue,
    /// The short status label shown next to the dot.
    pub status: AttrValue,
    /// An optional timestamp shown muted at the foot of the popover.
    #[prop_or_default]
    pub timestamp: Option<AttrValue>,
    /// Extra classes merged onto the popover (e.g. a width modifier).
    #[prop_or_default]
    pub class: Classes,
    /// The popover body.
    #[prop_or_default]
    pub children: Html,
}

#[function_component(Popover)]
pub fn popover(props: &PopoverProps) -> Html {
    html! {
        <div class={classes!("popover", props.align.class_name(), props.class.clone())}>
            <div class="popover__head">
                <StatusDot class={props.status_class.clone()} />
                <span class="popover__status">{&props.status}</span>
            </div>
            <div class="popover__body">
                { props.children.clone() }
            </div>
            if let Some(timestamp) = &props.timestamp {
                <div class="popover__time">{timestamp}</div>
            }
        </div>
    }
}
