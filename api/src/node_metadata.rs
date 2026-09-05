use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The well-known label under which a node publishes its hostname. Every other label is free-form
/// (`cloud`, `region`, `az`, `cluster`, ...), but this one has a dedicated role: it is what the UI
/// shows in place of the raw node identifier wherever a node is named.
pub const HOSTNAME_LABEL: &str = "hostname";

/// Descriptive metadata a node publishes about itself: a generic container of labels (its hostname,
/// the cloud/region/availability zone it runs in, the cluster it belongs to, ...), replicated through
/// the cluster gossip so every node can resolve a bare node identifier — as it appears in probe
/// `observers`/`observations` and on the cluster page — to something an operator recognises.
///
/// Returned by the operator-only `/api/v1/admin/cluster/nodes` endpoints. It describes machines and
/// deployment topology, so it is never surfaced to anonymous viewers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeMetadata {
    /// The node identifier this metadata describes.
    pub id: String,

    /// The node's labels, keyed by label name. Sorted so the wire and display order is stable.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,

    /// When the publishing node last (re-)stamped this record; the record's replication version.
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub last_updated: DateTime<Utc>,
}

impl NodeMetadata {
    pub fn new(
        id: impl Into<String>,
        labels: BTreeMap<String, String>,
        last_updated: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            labels,
            last_updated,
        }
    }

    /// The value of one label, if the node publishes it.
    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels.get(key).map(String::as_str)
    }

    /// The node's hostname, when it publishes one (a blank value counts as absent).
    pub fn hostname(&self) -> Option<&str> {
        self.label(HOSTNAME_LABEL).filter(|h| !h.trim().is_empty())
    }

    /// The name to show for this node: its hostname when known, otherwise the bare identifier.
    pub fn display_name(&self) -> &str {
        self.hostname().unwrap_or(&self.id)
    }

    /// The labels worth rendering as tags alongside the name — everything except the hostname,
    /// which is already shown as the name itself.
    pub fn tags(&self) -> impl Iterator<Item = (&str, &str)> {
        self.labels
            .iter()
            .filter(|(key, _)| key.as_str() != HOSTNAME_LABEL)
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(labels: &[(&str, &str)]) -> NodeMetadata {
        NodeMetadata::new(
            "1p3x9k",
            labels.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        )
    }

    #[test]
    fn display_name_prefers_the_hostname_and_falls_back_to_the_id() {
        assert_eq!(metadata(&[("hostname", "grey-syd-1")]).display_name(), "grey-syd-1");
        assert_eq!(metadata(&[]).display_name(), "1p3x9k");
        assert_eq!(metadata(&[("hostname", "  ")]).display_name(), "1p3x9k");
        assert_eq!(metadata(&[("region", "au-east")]).hostname(), None);
    }

    #[test]
    fn tags_exclude_the_hostname_and_are_ordered() {
        let m = metadata(&[("region", "au-east"), ("hostname", "grey-syd-1"), ("cloud", "aws")]);
        let tags: Vec<_> = m.tags().collect();
        assert_eq!(tags, vec![("cloud", "aws"), ("region", "au-east")]);
        assert_eq!(m.label("cloud"), Some("aws"));
        assert_eq!(m.label("az"), None);
    }

    #[test]
    fn serialises_with_a_stable_shape_and_round_trips() {
        let m = metadata(&[("hostname", "grey-syd-1"), ("region", "au-east")]);
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json["id"], "1p3x9k");
        assert_eq!(json["labels"]["hostname"], "grey-syd-1");
        assert_eq!(json["last_updated"], 1_700_000_000_000i64);
        assert_eq!(serde_json::from_value::<NodeMetadata>(json).unwrap(), m);

        // Records written before any labels existed decode with an empty label set.
        let bare: NodeMetadata =
            serde_json::from_str(r#"{"id":"x","last_updated":0}"#).unwrap();
        assert!(bare.labels.is_empty());

        let packed = rmp_serde::to_vec_named(&m).unwrap();
        assert_eq!(rmp_serde::from_slice::<NodeMetadata>(&packed).unwrap(), m);
    }
}
