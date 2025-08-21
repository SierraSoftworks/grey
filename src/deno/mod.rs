mod exts;
mod fake_fs;
mod module_loader;
mod worker;

pub use exts::grey_extension;
pub use worker::run_probe_script;

static RUNTIME_SNAPSHOT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/RUNTIME_SNAPSHOT.bin"));

pub fn runtime_snapshot() -> &'static [u8] {
    RUNTIME_SNAPSHOT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SampleValue;

    #[tokio::test]
    async fn test_runtime_snapshot() {

        let output = run_probe_script("console.log('we have console.log!!!');", Vec::new()).await.expect("script should run successfully");
        assert_eq!(output.get("exit_code"), &SampleValue::Int(0));
    }
}
