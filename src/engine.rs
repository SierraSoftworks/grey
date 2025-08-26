use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::{atomic::AtomicBool, Arc};

use tracing_batteries::prelude::opentelemetry::trace::SpanKind as OpenTelemetrySpanKind;
use tracing_batteries::prelude::*;

use crate::state::State;
use crate::probe_runner::ProbeRunner;
use crate::Probe;

pub struct Engine {
    state: State,
    probes: Arc<RwLock<HashMap<String, Arc<ProbeRunner>>>>,
}

impl Engine {
    pub fn new(state: State) -> Self {
        let probes: HashMap<String, Arc<ProbeRunner>> = state
            .get_config()
            .probes
            .iter()
            .map(|probe| {
                (
                    probe.name.clone(),
                    Self::build_probe_runner(&state, probe),
                )
            })
            .collect();

        Self {
            state,
            probes: Arc::new(RwLock::new(probes)),
        }
    }

    #[tracing::instrument(name = "engine", skip(self), fields(otel.kind=?OpenTelemetrySpanKind::Internal), err(Debug))]
    pub async fn run(&self, cancel: &AtomicBool) -> Result<(), Box<dyn std::error::Error>> {
        // Ensure that the state directory is created (if specified)
        if let Some(state_dir) = &self.state.get_config().state_directory {
            std::fs::create_dir_all(state_dir)?;
        }

        // Start config reload watcher
        self.start_config_reloader();

        // Start probe runners
        for probe in self.probes.read().unwrap().values().cloned() {
            self.start_probe_runner(probe);
        }

        if self.state.get_config().ui.enabled {
            eprintln!(
                "Starting web UI on http://{}",
                self.state.get_config().ui.listen.as_str()
            );

            crate::ui::start_server(self.state.clone()).await?;
        } else {
            while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
        }

        self.stop_all_probe_runners();

        Ok(())
    }

    fn build_probe_runner(state: &State, probe: &Probe) -> Arc<ProbeRunner> {
        let runner = if let Some(state_dir) = &state.get_config().state_directory {
            // If a state directory is configured, use it
            match ProbeRunner::with_snapshot_history(probe.clone(), state_dir.join(format!("{}.dat", probe.name))) {
                Ok(runner) => Arc::new(runner),
                Err(e) => {
                    warn!("Failed to create probe runner with snapshot history for '{}': {}. Using default state (no history).", probe.name, e);
                    Arc::new(ProbeRunner::new(probe.clone()))
                }
            }
        } else {
            Arc::new(ProbeRunner::new(probe.clone()))
        };

        state.with_default_history(probe.name.clone(), runner.history());

        runner
    }

    fn start_probe_runner(&self, probe: Arc<ProbeRunner>) {
        tokio::spawn(async move {
            if let Err(e) = probe.schedule().await {
                error!(name: "engine.probe", { probe.name=%probe.name(), action = "schedule", exception = e }, "Failed to schedule probe {}: {}", probe.name(), e);
            }
        });
    }

    fn stop_all_probe_runners(&self) {
        for probe in self.probes.read().unwrap().values().cloned() {
            probe.cancel();
        }
    }

    fn start_config_reloader(&self) {
        let state = self.state.clone();
        let probes = self.probes.clone();
        tokio::spawn(async move {
            let mut current_probes = state.get_config().probes.clone();
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                if let Err(err) = state.reload().await {
                    error!("Failed to reload config: {}", err);
                }

                let new_probes = state.get_config().probes.clone();
                if new_probes != current_probes {
                    let old_probes: HashMap<&str, &Probe> = current_probes
                        .iter()
                        .map(|p| (p.name.as_str(), p))
                        .collect();
                    let new_probes: HashMap<&str, &Probe> =
                        new_probes.iter().map(|p| (p.name.as_str(), p)).collect();

                    for (name, old_probe) in old_probes.iter() {
                        if let Some(new_probe) = new_probes.get(name) {
                            if old_probe != new_probe {
                                // Probe configuration has changed
                                info!(name: "config.reload.probe", { probe.name=name, action = "update" }, "Reloaded configuration for probe {}", name);
                                probes.read().unwrap().get(*name).map(|p| p.update((*new_probe).clone()));
                            }
                        } else {
                            // Probe has been removed
                            info!(name: "config.reload.probe", { probe.name=name, action = "remove" }, "Removed configuration for probe {}", name);
                            probes.read().unwrap().get(*name).map(|p| p.cancel());
                        }
                    }

                    for (name, new_probe) in new_probes {
                        if !old_probes.contains_key(name) {
                            // New probe has been added
                            let name = name.to_string();
                            info!(name: "config.reload.probe", { probe.name=name, action = "add" }, "Added configuration for probe {}", name);
                            let probe = Self::build_probe_runner(&state, new_probe);

                            state.with_default_history(probe.name(), probe.history());

                            probes.write().unwrap().insert(
                                name.to_string(),
                                probe.clone(),
                            );

                            tokio::spawn(async move {
                                if let Err(e) = probe.schedule().await {
                                    error!(name: "config.reload.probe", { probe.name=name, action = "schedule", exception = e }, "Failed to schedule probe {}: {}", name, e);
                                }
                            });
                        }
                    }
                }

                current_probes = new_probes;
            }
        });
    }
}
