use grey_api::Impact;

/// The colour class for an impact (`ok`/`warn`/`error`; `draft` is the muted hidden state).
pub fn impact_class(impact: Impact) -> &'static str {
    match impact {
        Impact::Offline => "error",
        Impact::Degraded => "warn",
        Impact::None => "ok",
        Impact::Hidden => "draft",
    }
}
