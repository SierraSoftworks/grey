use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::{Arc, atomic::AtomicBool};

use tracing_batteries::prelude::opentelemetry::trace::SpanKind as OpenTelemetrySpanKind;
use tracing_batteries::prelude::*;

use crate::probe_runner::ProbeRunner;
use crate::state::State;
use crate::{Probe, cluster};

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
                    Arc::new(ProbeRunner::new(probe.clone(), state.clone())),
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
        if let Some(state_dir) = &self.state.get_config().state.parent() {
            std::fs::create_dir_all(state_dir)?;
        }

        // Start config reload watcher
        self.start_config_reloader();

        {
            let state = self.state.clone();
            tokio::task::spawn_local(async move {
                state.gc_loop().await;
            });
        }

        // Start probe runners
        for probe in self.probes.read().unwrap().values().cloned() {
            self.start_probe_runner(probe);
        }

        if self.state.get_config().cluster.enabled {
            let config = self.state.get_config();
            let members = self.state.members();

            // Determine the address we advertise to peers for discovery: the configured
            // `advertised_address`, otherwise the listen address when it is concrete (non-wildcard).
            // A wildcard listener with no advertised address self-advertises nothing — peers still
            // learn us from the source address of our packets, but we are not discovered transitively.
            let advertised = config.cluster.advertised_address.clone().or_else(|| {
                match config.cluster.listen.parse::<std::net::SocketAddr>() {
                    Ok(addr) if !addr.ip().is_unspecified() => Some(config.cluster.listen.clone()),
                    _ => None,
                }
            });
            match advertised {
                Some(addr) => members.set_self_addresses(vec![addr]),
                None => warn!(
                    name: "cluster.advertise",
                    "No advertised_address is configured and the listen address is a wildcard; this node will not advertise an address for transitive discovery. Set cluster.advertised_address to a routable host:port to enable it."
                ),
            }

            let cluster_transport = cluster::UdpGossipTransport::new(
                &config.cluster.listen,
                cluster::Aes256Gcm,
                self.state.clone(),
            )
            .await?
            .with_max_message_size(config.cluster.max_message_size);
            let cluster_client =
                cluster::GossipClient::new(self.state.clone(), cluster_transport, members)
                    .with_gossip_factor(config.cluster.gossip_factor)
                    .with_gossip_interval(config.cluster.gossip_interval)
                    .with_membership_sample_size(config.cluster.membership_sample_size)
                    .with_seed_resolve_interval(config.cluster.peer_resolve_interval)
                    .with_seed_peers(config.cluster.peers.clone());

            tokio::task::spawn_local(async move {
                cluster_client.run().await;
            });
        }

        if self.state.get_config().ui.enabled {
            eprintln!(
                "Starting web UI on http://{}",
                self.state.get_config().ui.listen.as_str()
            );

            crate::api::start_server(self.state.clone()).await?;
        } else {
            while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
        }

        self.stop_all_probe_runners();

        Ok(())
    }

    fn start_probe_runner(&self, probe: Arc<ProbeRunner>) {
        tokio::task::spawn_local(async move {
            if let Err(e) = probe.schedule().await {
                error!(name: "engine.probe", { probe.name=%probe.name(), action = "schedule", exception = e }, "Failed to schedule probe {}: {}", probe.name(), e);
            }
        });
    }

    fn stop_all_probe_runners(&self) {
        for probe in self.probes.read().unwrap().values() {
            probe.cancel();
        }
    }

    fn start_config_reloader(&self) {
        let state = self.state.clone();
        let probes = self.probes.clone();
        tokio::task::spawn_local(async move {
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
                                if let Some(p) = probes.read().unwrap().get(*name) {
                                    p.update((*new_probe).clone());
                                    if let Err(err) = state.update_probe_config(*new_probe).await {
                                        error!(name: "config.reload.probe", { probe.name=name, action = "update", exception = err }, "Failed to update stored configuration for probe '{name}'");
                                    }
                                }
                            }
                        } else {
                            // Probe has been removed
                            info!(name: "config.reload.probe", { probe.name=name, action = "remove" }, "Removed configuration for probe {}", name);
                            if let Some(p) = probes.read().unwrap().get(*name) {
                                p.cancel()
                            }
                        }
                    }

                    for (name, new_probe) in new_probes {
                        if !old_probes.contains_key(name) {
                            // New probe has been added
                            let name = name.to_string();
                            info!(name: "config.reload.probe", { probe.name=name, action = "add" }, "Added configuration for probe {}", name);
                            let probe =
                                Arc::new(ProbeRunner::new(new_probe.clone(), state.clone()));

                            probes
                                .write()
                                .unwrap()
                                .insert(name.to_string(), probe.clone());

                            tokio::task::spawn_local(async move {
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
