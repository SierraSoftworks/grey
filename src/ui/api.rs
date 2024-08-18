use tide::{prelude::*, Request};

use super::State;

pub async fn get_probes(req: Request<State>) -> tide::Result {
    let state = req.state();
    let probes = state
        .probes
        .values()
        .map(|probe| probe.as_ref())
        .collect::<Vec<_>>();
    Ok(json!(probes).into())
}

pub async fn get_history(req: Request<State>) -> tide::Result {
    let probe = req.param("probe")?;
    let state = req.state();

    let probe = state
        .probes
        .get(probe)
        .ok_or_else(|| tide::Error::from_str(404, "Probe not found"))?;

    match probe.history.read() {
        Ok(history) => Ok(json!(history.iter().cloned().collect::<Vec<_>>()).into()),
        Err(_) => Err(tide::Error::from_str(
            500,
            "Failed to read history (lock is poisoned)",
        )),
    }
}
