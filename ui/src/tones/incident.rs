use grey_api::Impact;
use yewi::Tone;

/// The tone for an impact. A hidden (draft) update is `Neutral`, so it reads as muted.
pub fn impact_tone(impact: Impact) -> Tone {
    match impact {
        Impact::Offline => Tone::Danger,
        Impact::Degraded => Tone::Warn,
        Impact::None => Tone::Ok,
        Impact::Hidden => Tone::Neutral,
    }
}
