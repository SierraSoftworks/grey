//! The incident event timelines:
//! - [`HorizontalTimeline`] — the compact, full-width bar of impact-coloured markers used in
//!   landing-page summaries, each revealing its update in a popover on hover.
//! - [`VerticalTimeline`] — the full vertical event list used inside an [`crate::components::IncidentBlock`].

use crate::formatters::time_format;
use crate::tones::impact_tone;
use grey_api::{IncidentUpdate, IncidentView};
use yew::prelude::*;
use yewi::prelude::*;

/// An update's message, written in Markdown by an administrator. Raw HTML in it is shown as the text
/// that was written, never interpreted.
pub fn message(update: &IncidentUpdate) -> Html {
    html! { <Markdown source={update.message.clone()} top_heading={HeadingLevel::H4} /> }
}

/// The horizontal timeline used in summaries: the updates' impact-coloured markers sit on a rail
/// which takes each update's colour up to the next, and fades out at both ends to imply that time
/// led up to, and continued past, the events.
#[derive(Properties, PartialEq)]
pub struct HorizontalTimelineProps {
    pub incident: IncidentView,
}

#[function_component(HorizontalTimeline)]
pub fn horizontal_timeline(props: &HorizontalTimelineProps) -> Html {
    let updates = props.incident.sorted_updates();
    if updates.is_empty() {
        return html! {};
    }

    html! {
        <Timeline orientation={Orientation::Horizontal} label="Updates" class="incident__rail">
            { for updates.iter().map(|update| html! {
                <TimelineItem
                    key={update.id.to_string()}
                    tone={impact_tone(update.impact)}
                    title={html! { { update.impact.label() } }}
                    meta={html! { { time_format(update.timestamp) } }}
                >
                    { message(update) }
                </TimelineItem>
            }) }
        </Timeline>
    }
}

/// The vertical event timeline, most recent update first: each update is a marker (coloured by
/// impact) with its impact, timestamp and message. The rail below a marker keeps its colour down to
/// the next one.
#[derive(Properties, PartialEq)]
pub struct VerticalTimelineProps {
    pub updates: Vec<IncidentUpdate>,
}

#[function_component(VerticalTimeline)]
pub fn vertical_timeline(props: &VerticalTimelineProps) -> Html {
    let mut updates = props.updates.clone();
    updates.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    if updates.is_empty() {
        return html! { <Text tag={TextTag::P} color={TextColor::Muted}>{ "No updates yet." }</Text> };
    }

    html! {
        <Timeline label="Updates" run_off={TimelineRunOff::End}>
            { for updates.iter().map(|update| html! {
                <TimelineItem
                    key={update.id.to_string()}
                    tone={impact_tone(update.impact)}
                    title={html! { { update.impact.label() } }}
                    aside={html! {
                        <time datetime={update.timestamp.to_rfc3339()}>{ time_format(update.timestamp) }</time>
                    }}
                >
                    { message(update) }
                </TimelineItem>
            }) }
        </Timeline>
    }
}
