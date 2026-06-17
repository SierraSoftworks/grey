//! Small inline SVG icons (Feather-style line icons, `stroke = currentColor` so they take the
//! surrounding text colour). The markup is a set of compile-time constants authored here — never user
//! input — so rendering it via [`Html::from_html_unchecked`] carries no injection risk, and it renders
//! identically under SSR and in the browser (the same mechanism the markdown renderer relies on).

use yew::{AttrValue, Html};

fn icon(raw: &'static str) -> Html {
    Html::from_html_unchecked(AttrValue::Static(raw))
}

/// A floppy-disk "save" glyph, used as the unsaved-changes indicator.
pub fn save_icon() -> Html {
    icon(
        r#"<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"></path><polyline points="17 21 17 13 7 13 7 21"></polyline><polyline points="7 3 7 8 15 8"></polyline></svg>"#,
    )
}

/// A pencil "edit" glyph.
pub fn edit_icon() -> Html {
    icon(
        r#"<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z"></path></svg>"#,
    )
}

/// A check-mark "done" glyph, shown while a message is being edited.
pub fn check_icon() -> Html {
    icon(
        r#"<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="20 6 9 17 4 12"></polyline></svg>"#,
    )
}

/// A warning-triangle outline glyph, used by the "Declare Incident" action.
pub fn warning_icon() -> Html {
    icon(
        r#"<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"></path><line x1="12" y1="9" x2="12" y2="13"></line><line x1="12" y1="17" x2="12.01" y2="17"></line></svg>"#,
    )
}

/// An "X" glyph, used to dismiss the error banner.
pub fn close_icon() -> Html {
    icon(
        r#"<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>"#,
    )
}
