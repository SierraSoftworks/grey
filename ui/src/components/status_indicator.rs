use chrono::TimeDelta;
use yew::prelude::*;

#[derive(Clone, PartialEq)]
pub enum StatusLevel {
    Good,
    Warning,
    Error,
}

impl StatusLevel {
    fn class_name(&self) -> &'static str {
        match self {
            StatusLevel::Good => "good",
            StatusLevel::Warning => "warning",
            StatusLevel::Error => "error",
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct StatusIndicatorProps {
    pub last_update: chrono::DateTime<chrono::Utc>,
}

pub struct StatusIndicator {
    last_updated: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
}

impl Component for StatusIndicator {
    type Message = ();
    type Properties = StatusIndicatorProps;

    fn create(ctx: &Context<Self>) -> Self {
        Self {
            last_updated: ctx.props().last_update,
            now: chrono::Utc::now(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, _msg: Self::Message) -> bool {
        self.now = chrono::Utc::now();
        self.last_updated = ctx.props().last_update;

        #[cfg(feature = "wasm")]
        ctx.link().send_future(async move {
            gloo::timers::future::sleep(std::time::Duration::from_secs(1)).await;
        });

        true
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        let time_since_last_update = self.now - self.last_updated;

        let (status_level, time_text) = if time_since_last_update < TimeDelta::seconds(60) {
            (
                StatusLevel::Good,
                format!("{} secs", time_since_last_update.num_seconds()),
            )
        } else if time_since_last_update < TimeDelta::seconds(120) {
            (
                StatusLevel::Warning,
                format!("{} secs", time_since_last_update.num_seconds()),
            )
        } else {
            (
                StatusLevel::Error,
                format!("{} secs", time_since_last_update.num_seconds()),
            )
        };

        html! {
            <div class={format!("status-indicator {}", status_level.class_name())}>
                <div class="status-dot"></div>
                <span class="status-text">{time_text}</span>
            </div>
        }
    }
}
