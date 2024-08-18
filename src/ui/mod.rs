use std::{collections::HashMap, sync::Arc};

use crate::Probe;

mod api;
mod page;

#[derive(Clone)]
pub struct State {
    pub probes: HashMap<String, Arc<Probe>>,
}

pub fn new(probes: Vec<Arc<Probe>>) -> tide::Server<State> {
    let mut state = State {
        probes: HashMap::new(),
    };

    for probe in probes {
        state.probes.insert(probe.name.clone(), probe);
    }

    let mut app = tide::Server::with_state(state);

    app.at("/").get(page::index);

    app.at("/api/v1/probes").get(api::get_probes);

    app.at("/api/v1/probes/:probe/history")
        .get(api::get_history);

    app
}
