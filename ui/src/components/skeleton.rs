//! Loading placeholders that mirror the shape of the content they stand in for, so a page reserves
//! its eventual layout (no reflow) and signals "loading" with a shimmer instead of a bare spinner.
//! Modelled on Element Plus's skeleton: animated grey blocks in the rough silhouette of the data.

use yew::prelude::*;

/// A single shimmering placeholder bar. `width` is any CSS length (e.g. `"40%"`, `"8rem"`); omit it
/// to fill the line.
#[derive(Properties, PartialEq)]
pub struct SkeletonBarProps {
    #[prop_or_default]
    pub width: Option<AttrValue>,
    /// Extra classes for one-off sizing (e.g. a taller title bar).
    #[prop_or_default]
    pub class: Classes,
}

#[function_component(SkeletonBar)]
pub fn skeleton_bar(props: &SkeletonBarProps) -> Html {
    let style = props
        .width
        .as_ref()
        .map(|w| format!("width: {w};"))
        .unwrap_or_default();
    html! {
        <span class={classes!("skeleton__bar", props.class.clone())} style={style} aria-hidden="true"></span>
    }
}

/// A placeholder standing in for one [`crate::components::IncidentBlock`]: a header (timestamp,
/// title and status pill) over a vertical timeline of a few update cards.
#[function_component(IncidentBlockSkeleton)]
pub fn incident_block_skeleton() -> Html {
    html! {
        <article class="incident-block skeleton" aria-hidden="true">
            <div class="incident-block__header">
                <div class="incident-block__heading">
                    <h3 class="incident-block__title skeleton__title-row">
                        <SkeletonBar class={classes!("skeleton__timestamp")} width="4.5rem" />
                        <SkeletonBar class={classes!("skeleton__title")} width="14rem" />
                    </h3>
                </div>
                <SkeletonBar class={classes!("skeleton__pill")} width="5rem" />
            </div>
            <ul class="incident-timeline">
                { for (0..3).map(|i| html! { <IncidentTimelineItemSkeleton key={i} /> }) }
            </ul>
        </article>
    }
}

/// One row of the incident timeline placeholder: the rail circle/tail plus a stand-in update card.
#[function_component(IncidentTimelineItemSkeleton)]
fn incident_timeline_item_skeleton() -> Html {
    html! {
        <li class="incident-timeline__item">
            <div class="incident-timeline__rail">
                <span class="incident-timeline__circle skeleton__circle"></span>
                <span class="incident-timeline__tail skeleton__tail"></span>
            </div>
            <div class="incident-timeline__body">
                <SkeletonBar class={classes!("skeleton__time")} width="7rem" />
                <div class="incident-timeline__card skeleton__card">
                    <SkeletonBar width="100%" />
                    <SkeletonBar width="85%" />
                    <SkeletonBar width="60%" />
                </div>
            </div>
        </li>
    }
}
