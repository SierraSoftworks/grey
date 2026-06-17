use pulldown_cmark::{Event, Options, Parser, html};
use yew::prelude::*;

/// Renders markdown to sanitized HTML for display.
///
/// Incident content is authored by authenticated administrators, but we still neutralize any raw
/// HTML embedded in the markdown (turning it into escaped text) before handing the result to
/// [`Html::from_html_unchecked`]. `from_html_unchecked` performs no sanitization of its own, so this
/// keeps it from becoming an injection vector — defence in depth.
pub fn render_markdown(input: &str) -> Html {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);

    let parser = Parser::new_ext(input, options).map(|event| match event {
        // Block- and inline-level raw HTML are downgraded to text so the renderer escapes them.
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        other => other,
    });

    let mut rendered = String::new();
    html::push_html(&mut rendered, parser);

    Html::from_html_unchecked(AttrValue::from(rendered))
}
