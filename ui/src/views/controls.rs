//! A debug-only gallery of every UI control, rendered across its meaningful variations so visual
//! regressions (contrast, clipping, spacing) can be spotted at a glance on one page.
//!
//! Reached at `/controls`, and compiled only under `debug_assertions`, so release builds carry
//! neither the route nor the fixtures it renders. Specimens are deliberately static: they render
//! from the [`crate::demo`] fixtures rather than live data so the page looks identical on every load.

use yew::prelude::*;

use crate::components::icons;
use crate::components::{
    Banner, BannerKind, Cron as CronComponent, EmptyState, IncidentBlock, IncidentBlockSkeleton,
    IncidentsSection, Popover, PopoverAlign, Probe as ProbeComponent, StatusDot,
};

/// The status colour vocabulary shared by dots, popovers and badges. `ok`/`warn`/`error`/`unknown`
/// additionally have `.status` glyphs; `running` and `draft` are dot-only.
const STATUS_CLASSES: [&str; 6] = ["ok", "warn", "error", "running", "unknown", "draft"];

/// The status classes that have a `.status::before` glyph defined in the stylesheet.
const GLYPH_CLASSES: [&str; 4] = ["ok", "warn", "error", "unknown"];

#[function_component(ControlsView)]
pub fn controls_view() -> Html {
    html! {
        <div class="page controls">
            <h1>{"Controls"}</h1>
            <p class="controls__intro">
                {"Every control, in every state it can reach. Debug builds only — this page is not compiled into release builds."}
            </p>

            { status_glyphs_section() }
            { banners_section() }
            { status_dots_section() }
            { buttons_section() }
            { icons_section() }
            { popovers_section() }
            { empty_state_section() }
            { skeleton_section() }
            { probes_section() }
            { crons_section() }
            { incidents_section() }
        </div>
    }
}

/// The `.status` glyphs on every background they are used against. The filled variants are the hard
/// case: the glyph sits on a background of its own status colour, so it can only read against it via
/// the inherited text colour.
fn status_glyphs_section() -> Html {
    section(
        "Status glyphs",
        "`.status` on filled, tinted and plain backgrounds. Check the glyph reads against the fill and that its centre of mass sits on the middle of the label.",
        html! {
            <>
                <div class="controls__stack">
                    { for GLYPH_CLASSES.iter().map(|class| html! {
                        <div class={classes!("section", "fill", *class)}>
                            <span class={classes!("status", *class)}>{format!("Filled background — .section.fill.{class}")}</span>
                        </div>
                    }) }
                </div>

                <div class="controls__stack controls__stack--large">
                    { for GLYPH_CLASSES.iter().map(|class| html! {
                        <div class={classes!("section", "fill", *class)}>
                            <span class={classes!("status", *class)}>{*class}</span>
                        </div>
                    }) }
                </div>

                <div class="controls__stack controls__stack--boxed">
                    { for GLYPH_CLASSES.iter().map(|class| html! {
                        <div class={classes!("section", *class)}>
                            <span class={classes!("status", *class)}>{format!("Box background — .section.{class}")}</span>
                        </div>
                    }) }
                </div>

                <div class="controls__row">
                    { for GLYPH_CLASSES.iter().map(|class| html! {
                        <span class={classes!("status", *class)}>{*class}</span>
                    }) }
                </div>

                <div class="controls__row controls__row--large">
                    { for GLYPH_CLASSES.iter().map(|class| html! {
                        <span class={classes!("status", *class)}>{*class}</span>
                    }) }
                </div>
            </>
        },
    )
}

fn banners_section() -> Html {
    section(
        "Banner",
        "The landing-page top-line status.",
        html! {
            <div class="controls__stack">
                <Banner kind={BannerKind::Ok} text="All services operating normally" />
                <Banner kind={BannerKind::Warning} text="Some services are degraded" />
                <Banner kind={BannerKind::Error} text="A major outage is in progress" />
            </div>
        },
    )
}

fn status_dots_section() -> Html {
    section(
        "Status dot",
        "Every colour, idle and pulsing, at the sizes used across the UI.",
        html! {
            <>
                <div class="controls__grid">
                    { for STATUS_CLASSES.iter().map(|class| specimen(class, html! {
                        <span class="controls__dots">
                            <StatusDot class={*class} />
                            <StatusDot class={*class} active=true />
                        </span>
                    })) }
                </div>
                <div class="controls__grid">
                    { for [6usize, 8, 12, 20].iter().map(|size| specimen(&format!("{size}px"), html! {
                        <span class="controls__dots">
                            { for STATUS_CLASSES.iter().map(|class| html! {
                                <StatusDot class={*class} size={*size} />
                            }) }
                        </span>
                    })) }
                </div>
            </>
        },
    )
}

fn buttons_section() -> Html {
    section(
        "Buttons",
        "Shared button treatments, including their disabled state.",
        html! {
            <div class="controls__grid">
                { specimen(".auth-button", html! { <button class="auth-button">{"Sign in"}</button> }) }
                { specimen(".primary-button", html! { <button class="primary-button">{"Save changes"}</button> }) }
                { specimen(".primary-button:disabled", html! { <button class="primary-button" disabled=true>{"Save changes"}</button> }) }
                { specimen(".declare-incident", html! {
                    <a class="declare-incident" href="#controls">
                        { icons::warning_icon() }
                        <span>{"Declare Incident"}</span>
                    </a>
                }) }
                { specimen(".link-button", html! { <button class="link-button">{"Edit"}</button> }) }
                { specimen(".link-button.danger", html! { <button class="link-button danger">{"Remove"}</button> }) }
            </div>
        },
    )
}

fn icons_section() -> Html {
    section(
        "Icons",
        "Inline SVG glyphs; each takes the surrounding text colour.",
        html! {
            <div class="controls__grid">
                { specimen("save", icons::save_icon()) }
                { specimen("edit", icons::edit_icon()) }
                { specimen("trash", icons::trash_icon()) }
                { specimen("check", icons::check_icon()) }
                { specimen("warning", icons::warning_icon()) }
                { specimen("close", icons::close_icon()) }
            </div>
        },
    )
}

/// Popovers normally mount on hover; here they are pinned open so all three alignments can be
/// compared at once. They anchor above their trigger, so the row reserves space overhead.
fn popovers_section() -> Html {
    section(
        "Popover",
        "Pinned open. Each anchors above its trigger, with the arrow pointing at it.",
        html! {
            <div class="controls__popovers">
                { popover_specimen(PopoverAlign::Left, "left", "ok", "Healthy") }
                { popover_specimen(PopoverAlign::Center, "center", "warn", "Degraded") }
                { popover_specimen(PopoverAlign::Right, "right", "error", "Failed") }
            </div>
        },
    )
}

fn popover_specimen(align: PopoverAlign, label: &str, status_class: &str, status: &str) -> Html {
    html! {
        <div class="controls__popover-anchor">
            <Popover
                align={align}
                status_class={status_class.to_string()}
                status={status.to_string()}
                timestamp={"2 minutes ago"}
            >
                <div>{"http.status_code == 200"}</div>
                <div>{"Checked from grey-syd-1, grey-lhr-1 and grey-iad-1."}</div>
            </Popover>
            <span class="controls__popover-trigger">{label}</span>
        </div>
    }
}

fn empty_state_section() -> Html {
    section(
        "Empty state",
        "Shown when a page has loaded but has nothing to display.",
        html! {
            <>
                <EmptyState title="No incidents reported">
                    {"Everything has been operating normally. Incidents will appear here if a problem is reported."}
                </EmptyState>
                <EmptyState title="Nothing scheduled" />
                <p class="empty-state">{"Inline .empty-state text"}</p>
            </>
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

fn probes_section() -> Html {
    section(
        "Probe",
        "Solid, flaky, recovered and failing histories, with long names and many tags.",
        html! {
            <div class="controls__stack controls__stack--boxed">
                { for crate::demo::probes().into_iter().map(|probe| {
                    let key = probe.name.clone();
                    html! { <ProbeComponent key={key} probe={probe} /> }
                }) }
            </div>
        },
    )
}

fn crons_section() -> Html {
    section(
        "Cron",
        "Healthy, running, missed, failed and never-seen jobs.",
        html! {
            <div class="controls__stack controls__stack--boxed">
                { for crate::demo::crons().into_iter().map(|cron| {
                    let key = cron.name.clone();
                    html! { <CronComponent key={key} cron={cron} /> }
                }) }
            </div>
        },
    )
}

fn incidents_section() -> Html {
    let incidents = crate::demo::incidents();

    section(
        "Incidents",
        "The landing-page summaries (horizontal timeline) and the full blocks used on the incidents page.",
        html! {
            <>
                <div class="controls__stack controls__stack--boxed">
                    <IncidentsSection incidents={incidents.clone()} />
                </div>
                { for incidents.iter().map(|incident| html! {
                    <IncidentBlock key={incident.id().to_string()} incident={incident.clone()} />
                }) }
            </>
        },
    )
}

// --- Layout helpers -------------------------------------------------------------------------

fn section(title: &str, note: &str, body: Html) -> Html {
    // The id makes each section directly linkable (e.g. `/controls#status-dot`), which is how the
    // sections are navigated to when capturing screenshots.
    let id = title.to_lowercase().replace(' ', "-");

    html! {
        <section class="controls__section" id={id}>
            <h2 class="controls__section-title">{title}</h2>
            <p class="controls__section-note">{note}</p>
            <div class="controls__section-body">{body}</div>
        </section>
    }
}

fn specimen(label: &str, body: Html) -> Html {
    html! {
        <div class="controls__specimen">
            <div class="controls__specimen-body">{body}</div>
            <code class="controls__specimen-label">{label}</code>
        </div>
    }
}
