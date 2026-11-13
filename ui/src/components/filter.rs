//! The filter typed into the app bar, shared with the pages that narrow their lists by it.

use std::rc::Rc;

use yew::prelude::*;

/// The current filter text. An empty filter matches everything.
#[derive(Clone, PartialEq, Default)]
pub struct Filter {
    terms: Rc<[String]>,
}

impl Filter {
    pub fn new(query: &str) -> Self {
        Self { terms: query.split_whitespace().map(str::to_lowercase).collect() }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Whether every term appears in at least one of `fields`, ignoring case. Terms are matched
    /// independently, so `api eu-west` finds a probe named for one and tagged with the other.
    pub fn matches<'a>(&self, fields: impl IntoIterator<Item = &'a str> + Clone) -> bool {
        self.terms.iter().all(|term| {
            fields.clone().into_iter().any(|field| field.to_lowercase().contains(term.as_str()))
        })
    }
}

/// The filter of the page being shown; matches everything outside a [`crate::Shell`].
#[hook]
pub fn use_filter() -> Filter {
    use_context::<Filter>().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_term_must_match_some_field() {
        let fields = ["api.example.com/healthz", "Public API", "region:eu-west"];
        let cases = [
            ("", true),
            ("API", true),
            ("api eu-west", true),
            ("region:eu", true),
            ("api storage", false),
        ];
        for (query, expected) in cases {
            assert_eq!(Filter::new(query).matches(fields), expected, "query {query:?}");
        }
        assert!(Filter::new("  ").is_empty() && !Filter::new("api").is_empty(), "blank queries filter nothing");
    }
}
