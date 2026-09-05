use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::{Arc, atomic::AtomicBool};

use tracing_batteries::prelude::opentelemetry::trace::SpanKind as OpenTelemetrySpanKind;
use tracing_batteries::prelude::*;

use crate::probe_runner::ProbeRunner;
use crate::state::{NodeMetadataStore, ProbeStore, State};
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

        // Tombstone any stored records for probes that were removed from the configuration while this
        // node was down, so they stop appearing in the UI (and are dropped by our peers).
        if let Err(err) = self.state.reconcile_probe_config().await {
            error!(name: "engine.probes.reconcile", { exception = err }, "Failed to reconcile stored probe state with the configuration: {err}");
        }

        // Publish this node's metadata (hostname and configured labels) so peers can name it. The GC
        // loop keeps the record fresh from here on.
        if let Err(err) = self.state.refresh_node_metadata().await {
            error!(name: "engine.node_metadata", { exception = err }, "Failed to publish this node's metadata: {err}");
        }

        {
            let state = self.state.clone();
            info!(
                name: "engine.gc.start",
                {
                    gc.interval = ?state.get_config().cluster.gc_interval,
                    gc.expiry = ?state.get_config().cluster.gc_probe_expiry,
                },
                "Starting state garbage collector.",
            );
            tokio::task::spawn_local(async move {
                state.gc_loop().await;
            });
        }

        // Hot-path state writes commit with deferred durability; this periodically persists them.
        {
            let state = self.state.clone();
            tokio::task::spawn_local(async move {
                state.flush_loop().await;
            });
        }

        // Watch for probe/cron state transitions and deliver webhook notifications. Always started:
        // it continuously tracks the baseline state (cheaply when no webhooks are configured), so a
        // webhook added by a later config reload begins notifying on the next transition rather than
        // replaying everything already in flight.
        {
            let state = self.state.clone();
            info!(
                name: "engine.notifier.start",
                { webhooks = state.get_config().webhooks.len() },
                "Starting webhook notifier.",
            );
            tokio::task::spawn_local(async move {
                crate::notify::Notifier::new(state).run().await;
            });
        }

        // Materialise time-derived cron faults (missed/stuck runs) into persisted state, so they
        // surface as run placeholders in the UI and drive streak-based alerting like reported
        // failures. Runs alongside the notifier so a missed run's failing streak observation is in
        // place before the notifier evaluates it.
        {
            let state = self.state.clone();
            info!(
                name: "engine.cron_monitor.start",
                { crons = state.get_config().crons.len() },
                "Starting cron monitor.",
            );
            tokio::task::spawn_local(async move {
                crate::cron_monitor::CronMonitor::new(state).run().await;
            });
        }

        // Start probe runners
        let probes = self.probes.read().unwrap().values().cloned().collect::<Vec<_>>();
        info!(name: "engine.probes.start", { probes = probes.len() }, "Starting {} probe runner(s).", probes.len());
        for probe in probes {
            self.start_probe_runner(probe);
        }

        if self.state.get_config().cluster.enabled {
            let config = self.state.get_config();
            let members = self.state.members();

            // The advertised addresses are computed from the configuration when the membership
            // registry is constructed (see `State::new`). Without one, transitive discovery still
            // works as long as the source addresses other nodes observe for this node are reachable
            // cluster-wide; it only breaks down across network boundaries.
            if config.cluster.advertised_addresses().is_empty() {
                warn!(
                    name: "cluster.advertise",
                    "No advertised_address is configured and the listen address is a wildcard; peers will discover this node from the source address of its gossip messages. If the cluster spans multiple networks (e.g. a LAN and a WAN), set cluster.advertised_address to an address that is reachable from all of them."
                );
            }

            let cluster_transport = cluster::UdpGossipTransport::new(
                &config.cluster.listen,
                cluster::Aes256Gcm,
                self.state.clone(),
            )
            .await?
            .with_message_mtu(config.cluster.message_mtu);
            let cluster_client =
                cluster::GossipClient::new(self.state.clone(), cluster_transport, members)
                    .with_gossip_factor(config.cluster.gossip_factor)
                    .with_gossip_interval(config.cluster.gossip_interval)
                    .with_seed_resolve_interval(config.cluster.peer_resolve_interval)
                    .with_seed_peers(config.cluster.peers.clone());

            tokio::task::spawn_local(async move {
                cluster_client.run().await;
            });
        }

        if self.state.get_config().ui.enabled {
            info!(
                "Starting web UI on http://{}",
                self.state.get_config().ui.listen.as_str()
            );

            crate::api::start_server(self.state.clone()).await?;
        } else {
            while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
        }

        info!(name: "engine.stop", "Stopping probe runners.");
        self.stop_all_probe_runners();

        // Persist whatever the deferred-durability commits have written since the last flush.
        if let Err(err) = self.state.flush().await {
            error!(name: "engine.state.flush", { exception = err }, "Failed to flush state to disk on shutdown: {err}");
        }

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
                match state.reload().await {
                    // Re-publish the node's labels so a changed `cluster.labels` propagates now
                    // rather than on the next GC pass.
                    Ok(true) => {
                        if let Err(err) = state.refresh_node_metadata().await {
                            error!(name: "config.reload.node_metadata", { exception = err }, "Failed to republish this node's metadata: {err}");
                        }
                    }
                    Ok(false) => {}
                    Err(err) => error!("Failed to reload config: {}", err),
                }

                let new_probes = state.get_config().probes.clone();
                if new_probes != current_probes {
                    // Retire the records of removed probes (and revive those that came back) so the
                    // configuration remains the source of truth for what the UI shows.
                    if let Err(err) = state.reconcile_probe_config().await {
                        error!(name: "config.reload.probe", { action = "reconcile", exception = err }, "Failed to reconcile stored probe state with the configuration: {err}");
                    }

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
