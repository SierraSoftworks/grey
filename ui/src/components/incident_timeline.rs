//! The incident event timelines:
//! - [`HorizontalTimeline`] — the compact, full-width bar of impact-coloured markers used in
//!   landing-page summaries, each revealing its update in a popover on hover.
//! - [`VerticalTimeline`] — the full vertical event list used inside an [`crate::components::IncidentBlock`].

use crate::components::markdown::render_markdown;
use crate::components::{Popover, PopoverAlign};
use crate::formatters::time_format;
use crate::styles::impact_class;
use grey_api::{IncidentUpdate, IncidentView};
use yew::prelude::*;

/// The horizontal timeline used in summaries: a lead-in tail in the first update's colour runs in
/// from the left, the updates' impact-coloured dots sit on connecting segments, and a lead-out tail
/// in the last update's colour trails off to the right. The tails fade outward to imply that time
/// led up to, and continued past, the events. Popovers anchor inward at the ends so they stay
/// on-screen.
#[derive(Properties, PartialEq)]
pub struct HorizontalTimelineProps {
    pub incident: IncidentView,
}

#[function_component(HorizontalTimeline)]
pub fn horizontal_timeline(props: &HorizontalTimelineProps) -> Html {
    let active = use_state(|| Option::<usize>::None);
    let updates = props.incident.sorted_updates();
    if updates.is_empty() {
        return html! {};
    }
    let last = updates.len() - 1;

    html! {
        <div class="incident-hbar">
            // The lead-in tail carries the first update's colour, fading in from the left.
            <span class={classes!("incident-hbar__line", "lead-in", impact_class(updates[0].impact))}></span>
            { for updates.iter().enumerate().map(|(i, update)| {
                let dot_class = impact_class(update.impact);
                let is_open = *active == Some(i);
                let on_enter = { let active = active.clone(); Callback::from(move |_| active.set(Some(i))) };
                let on_leave = { let active = active.clone(); Callback::from(move |_| active.set(None)) };
                // Anchor popovers inward at the ends so they don't run off the page.
                let align = if i == 0 { PopoverAlign::Left } else if i == last { PopoverAlign::Right } else { PopoverAlign::Center };
                html! {
                    <>
                        // The line before a marker carries the preceding marker's colour.
                        if i > 0 {
                            <span class={classes!("incident-hbar__line", impact_class(updates[i - 1].impact))}></span>
                        }
                        <div
                            class={classes!("incident-hbar__dot-wrap", is_open.then_some("open"))}
                            onmouseenter={on_enter}
                            onmouseleave={on_leave}
                        >
                            <span class={classes!("incident-hbar__dot", dot_class)}></span>
                            if is_open {
                                <Popover
                                    {align}
                                    status_class={dot_class}
                                    status={update.impact.label()}
                                    timestamp={time_format(update.timestamp)}
                                >
                                    <div class="markdown">
                                        { render_markdown(&update.message) }
                                    </div>
                                </Popover>
                            }
                        </div>
                    </>
                }
            }) }
            // The lead-out tail carries the last update's colour, fading off to the right.
            <span class={classes!("incident-hbar__line", "lead-out", impact_class(updates[last].impact))}></span>
        </div>
    }
}

/// The vertical event timeline: each update is a circle (coloured by impact) with a card showing its
/// timestamp, impact and message. The line below a circle keeps its colour down to the next one.
#[derive(Properties, PartialEq)]
pub struct VerticalTimelineProps {
    pub updates: Vec<IncidentUpdate>,
}

#[function_component(VerticalTimeline)]
pub fn vertical_timeline(props: &VerticalTimelineProps) -> Html {
    let mut updates = props.updates.clone();
    // Most recent update first.
    updates.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    if updates.is_empty() {
        return html! { <p class="empty-state">{"No updates yet."}</p> };
    }

    html! {
        <ul class="incident-timeline">
            { for updates.iter().map(|update| {
                let class = impact_class(update.impact);
                html! {
                    <li class="incident-timeline__item">
                        <div class="incident-timeline__rail">
                            <span class={classes!("incident-timeline__circle", class)}></span>
                            <span class={classes!("incident-timeline__tail", class)}></span>
                        </div>
                        <div class="incident-timeline__body">
                            <div class="incident-timeline__time">
                                <span class={classes!("incident-status-pill", class)}>{update.impact.label()}</span>
                                <time datetime={update.timestamp.to_rfc3339()}>{time_format(update.timestamp)}</time>
                            </div>
                            <div class={classes!("incident-timeline__card", class)}>
                                <div class="incident-timeline__card-message markdown">{ render_markdown(&update.message) }</div>
                            </div>
                        </div>
                    </li>
                }
            }) }
        </ul>
    }
}
