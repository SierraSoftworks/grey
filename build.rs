use std::path::PathBuf;

#[cfg(feature = "scripts")]
mod runtime {
    use super::*;
    use deno_core::serde_v8::AnyValue;
    use deno_core::{op2, OpState};
    use deno_runtime::ops::bootstrap::SnapshotOptions;
    use deno_runtime::*;

    deno_core::extension!(
        grey_extension,
        ops = [
            op_set_output,
            op_get_trace_headers
        ],
        esm_entry_point = "ext:grey_extension/40_output.js",
        esm = [
            dir "src/deno/js",
            "40_output.js"
        ]
    );

    #[op2]
    fn op_set_output(
        _state: &mut OpState,
        #[string] _name: String,
        #[serde] _value: Option<AnyValue>,
    ) {
        // Implementation for setting output
    }

    #[op2]
    #[serde]
    fn op_get_trace_headers() -> std::collections::HashMap<String, String> {
        Default::default()
    }

    pub fn create_deno_snapshot(snapshot_path: PathBuf) {
        deno_runtime::snapshot::create_runtime_snapshot(
            snapshot_path,
            SnapshotOptions::default(),
            vec![grey_extension::init()],
        );
    }
}

fn main() {
    println!(
        "cargo:rustc-env=TARGET={}",
        std::env::var("TARGET").unwrap()
    );
    println!(
        "cargo:rustc-env=PROFILE={}",
        std::env::var("PROFILE").unwrap()
    );

    #[cfg(feature = "scripts")]
    {
        let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        runtime::create_deno_snapshot(out_dir.join("RUNTIME_SNAPSHOT.bin"));
    }
}
