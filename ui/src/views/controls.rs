//! A debug-only gallery of the controls that are Grey's own, rendered across their meaningful
//! variations so visual regressions (contrast, clipping, spacing) can be spotted at a glance on one
//! page. The generic controls (buttons, inputs, pills, popovers) belong to yewi and are covered by
//! its reference site.
//!
//! Reached at `/controls`, and compiled only under `debug_assertions`, so release builds carry
//! neither the route nor the fixtures it renders. Specimens are deliberately static: they render
//! from the [`crate::demo`] fixtures rather than live data so the page looks identical on every load.

use yew::prelude::*;
use yewi::prelude::*;

use crate::components::{
    Cron as CronComponent, IncidentBlock, IncidentBlockSkeleton, IncidentsSection,
    Probe as ProbeComponent,
};

#[function_component(ControlsView)]
pub fn controls_view() -> Html {
    html! {
        <>
            <PageHeader
                title="Controls"
                subtitle={html! { { "Grey's own controls, in every state they can reach. Debug builds only: this page is not compiled into release builds." } }}
            />
            <div class="controls">
                { banners_section() }
                { tones_section() }
                { probes_section() }
                { crons_section() }
                { incidents_section() }
                { skeleton_section() }
                { empty_state_section() }
            </div>
        </>
    }
}

fn banners_section() -> Html {
    section(
        "Banner",
        "The landing-page top-line status, in each state.",
        html! {
            <Stack gap={Space::Sm}>
                <Banner tone={Tone::Ok} bold=true size={Size::Large}>{ "All services operating normally" }</Banner>
                <Banner tone={Tone::Warn} bold=true size={Size::Large}>{ "Partial degradation in service" }</Banner>
                <Banner tone={Tone::Danger} bold=true size={Size::Large}>{ "Major outage affecting multiple services" }</Banner>
            </Stack>
        },
    )
}

/// Every tone the status vocabulary maps onto, as the dot and the pill it appears in.
fn tones_section() -> Html {
    let states = [
        (Tone::Ok, "ok"),
        (Tone::Warn, "warn"),
        (Tone::Danger, "error"),
        (Tone::Active, "running"),
        (Tone::Info, "transitive"),
        (Tone::Neutral, "unknown / draft"),
    ];

    section(
        "Status tones",
        "Each health state as a dot (idle and live) and as a pill.",
        html! {
            <Cluster gap={Space::Lg}>
                { for states.into_iter().map(|(tone, label)| html! {
                    <Cluster gap={Space::Xs}>
                        <StatusDot {tone} />
                        <StatusDot {tone} active=true />
                        <Pill {tone}>{ label }</Pill>
                    </Cluster>
                }) }
            </Cluster>
        },
    )
}

fn probes_section() -> Html {
    section(
        "Probe",
        "Solid, flaky, recovered and failing histories, with long names and many tags.",
        html! {
            <Card>
                { for crate::demo::probes().into_iter().map(|probe| {
                    let key = probe.name.clone();
                    html! { <ProbeComponent key={key} probe={probe} /> }
                }) }
            </Card>
        },
    )
}

fn crons_section() -> Html {
    section(
        "Cron",
        "Healthy, running, missed, failed and never-seen jobs.",
        html! {
            <Card>
                { for crate::demo::crons().into_iter().map(|cron| {
                    let key = cron.name.clone();
                    html! { <CronComponent key={key} cron={cron} /> }
                }) }
            </Card>
        },
    )
}

fn incidents_section() -> Html {
    let incidents = crate::demo::incidents();

    section(
        "Incidents",
        "The landing-page summaries (horizontal timeline) and the full blocks used on the incidents page.",
        html! {
            <div class="incidents">
                <IncidentsSection incidents={incidents.clone()} />
                { for incidents.iter().map(|incident| html! {
                    <IncidentBlock key={incident.id().to_string()} incident={incident.clone()} />
                }) }
            </div>
        },
    )
}

fn skeleton_section() -> Html {
    section(
        "Loading skeleton",
        "The placeholder an incident block collapses to while loading.",
        html! { <IncidentBlockSkeleton /> },
    )
}

fn empty_state_section() -> Html {
    section(
        "Empty state",
        "Shown when a page has loaded but has nothing to list.",
        html! {
            <EmptyState title="No incidents reported" icon={icons::CHECK} tone={Tone::Ok}>
                { "Everything has been operating normally. Incidents will appear here if a problem is reported." }
            </EmptyState>
        },
    )
}

fn section(title: &str, note: &str, body: Html) -> Html {
    // The id makes each section directly linkable (e.g. `/controls#probe`), which is how the
    // sections are navigated to when capturing screenshots.
    let id = title.to_lowercase().replace(' ', "-");

    html! {
        <section class="controls__section" {id}>
            <Heading level={HeadingLevel::H2} size={TextSize::Lg}>{ title.to_string() }</Heading>
            <Text tag={TextTag::P} color={TextColor::Muted}>{ note.to_string() }</Text>
            <div class="controls__body">{ body }</div>
        </section>
    }
}
