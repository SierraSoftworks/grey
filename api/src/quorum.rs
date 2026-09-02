use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How many agreeing observers are required before a pooled health decision is made.
///
/// A probe is observed by every node that runs it; a quorum decides how many of those observers
/// must independently report a (debounced) failure before the cluster reads the probe as failing,
/// and by symmetry how many must have stopped reporting one before it reads as recovered. The same
/// rule sizes the quorum of a node's own probes when deciding whether the *node* is degraded.
///
/// Serialised as `majority`, a plain count (`2`), or a percentage (`60%`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quorum {
    /// A strict majority of observers: `floor(n / 2) + 1`. A single observer is its own majority, so
    /// a standalone Grey instance behaves exactly as it did without quorums.
    #[default]
    Majority,
    /// A fixed number of observers, clamped to the number actually observing.
    Count(u32),
    /// A percentage of observers (rounded up), clamped to at least one.
    Percent(u8),
}

impl Quorum {
    /// The number of observers (out of `observers`) that must agree. Always at least 1 and, when
    /// there is at least one observer, never more than `observers` — so a quorum can always be met
    /// by unanimous agreement, and an over-specified count degrades to unanimity rather than to a
    /// probe that can never fail.
    pub fn required(self, observers: usize) -> usize {
        let required = match self {
            Quorum::Majority => observers / 2 + 1,
            Quorum::Count(count) => count as usize,
            Quorum::Percent(percent) => {
                (observers * percent.min(100) as usize).div_ceil(100)
            }
        };
        required.clamp(1, observers.max(1))
    }
}

impl std::fmt::Display for Quorum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Quorum::Majority => write!(f, "majority"),
            Quorum::Count(count) => write!(f, "{count}"),
            Quorum::Percent(percent) => write!(f, "{percent}%"),
        }
    }
}

impl std::str::FromStr for Quorum {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("majority") {
            return Ok(Quorum::Majority);
        }
        if let Some(percent) = s.strip_suffix('%') {
            return percent
                .trim()
                .parse::<u8>()
                .ok()
                .filter(|p| (1..=100).contains(p))
                .map(Quorum::Percent)
                .ok_or_else(|| format!("'{s}' is not a valid quorum percentage (expected 1% to 100%)"));
        }
        s.parse::<u32>()
            .ok()
            .filter(|c| *c >= 1)
            .map(Quorum::Count)
            .ok_or_else(|| format!("'{s}' is not a valid quorum (expected 'majority', a count, or a percentage)"))
    }
}

impl Serialize for Quorum {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Quorum::Count(count) => serializer.serialize_u32(*count),
            other => serializer.serialize_str(&other.to_string()),
        }
    }
}

impl<'de> Deserialize<'de> for Quorum {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Count(u64),
            Text(String),
        }

        match Repr::deserialize(deserializer)? {
            Repr::Count(count) => u32::try_from(count)
                .ok()
                .filter(|c| *c >= 1)
                .map(Quorum::Count)
                .ok_or_else(|| serde::de::Error::custom(format!("'{count}' is not a valid quorum count"))),
            Repr::Text(text) => text.parse().map_err(serde::de::Error::custom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_observers() {
        let cases = [
            (Quorum::Majority, 0, 1),
            (Quorum::Majority, 1, 1),
            (Quorum::Majority, 2, 2),
            (Quorum::Majority, 3, 2),
            (Quorum::Majority, 4, 3),
            (Quorum::Majority, 5, 3),
            (Quorum::Count(2), 1, 1),
            (Quorum::Count(2), 3, 2),
            (Quorum::Count(9), 3, 3),
            (Quorum::Percent(50), 3, 2),
            (Quorum::Percent(50), 4, 2),
            (Quorum::Percent(100), 4, 4),
            (Quorum::Percent(1), 4, 1),
            (Quorum::Percent(60), 0, 1),
        ];
        for (quorum, observers, expected) in cases {
            assert_eq!(quorum.required(observers), expected, "{quorum} of {observers}");
        }
    }

    #[test]
    fn parses_and_roundtrips() {
        for (text, expected) in [
            ("majority", Quorum::Majority),
            ("Majority", Quorum::Majority),
            ("2", Quorum::Count(2)),
            ("60%", Quorum::Percent(60)),
            (" 100% ", Quorum::Percent(100)),
        ] {
            assert_eq!(text.parse::<Quorum>().unwrap(), expected);
        }
        for bad in ["0", "0%", "101%", "most", "-1", ""] {
            assert!(bad.parse::<Quorum>().is_err(), "'{bad}' must be rejected");
        }

        for quorum in [Quorum::Majority, Quorum::Count(2), Quorum::Percent(60)] {
            let json = serde_json::to_string(&quorum).unwrap();
            assert_eq!(serde_json::from_str::<Quorum>(&json).unwrap(), quorum);
            let packed = rmp_serde::to_vec_named(&quorum).unwrap();
            assert_eq!(rmp_serde::from_slice::<Quorum>(&packed).unwrap(), quorum);
        }
        assert_eq!(serde_json::from_str::<Quorum>("2").unwrap(), Quorum::Count(2));
        assert_eq!(serde_json::from_str::<Quorum>("\"majority\"").unwrap(), Quorum::Majority);
        assert!(serde_json::from_str::<Quorum>("0").is_err());
    }
}
