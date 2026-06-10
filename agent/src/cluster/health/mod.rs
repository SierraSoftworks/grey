//! Peer health assessment for the cluster membership registry.
//!
//! Detection models live in their own submodules (currently [`phi`] for the phi-accrual detector)
//! so that alternative models can be added alongside without restructuring.

mod phi;

pub use phi::PhiAccrualDetector;

/// The liveness verdict for a peer, derived from the failure detector and the per-address
/// send/receive signals tracked in the membership registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// We are confident the peer is reachable.
    Healthy,
    /// The peer has been quiet for longer than expected; it may be failing.
    Suspect,
    /// The peer is considered failed (no observed heartbeats for a long time).
    Dead,
    /// The peer is online (its heartbeat is still advancing, as learned via other peers) but it is
    /// not responding to our messages. We cannot tell whether our messages are being received, only
    /// that no replies arrive over any of the addresses we have for it.
    Unreachable,
}

impl Liveness {
    pub fn as_str(&self) -> &'static str {
        match self {
            Liveness::Healthy => "healthy",
            Liveness::Suspect => "suspect",
            Liveness::Dead => "dead",
            Liveness::Unreachable => "unreachable",
        }
    }

    /// Whether this verdict warrants an operator warning (as opposed to a healthy/info state).
    pub fn is_degraded(&self) -> bool {
        !matches!(self, Liveness::Healthy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liveness_degraded_classification() {
        assert!(!Liveness::Healthy.is_degraded());
        assert!(Liveness::Suspect.is_degraded());
        assert!(Liveness::Dead.is_degraded());
        assert!(Liveness::Unreachable.is_degraded());
    }
}
