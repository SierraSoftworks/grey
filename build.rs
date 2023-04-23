use std::path::PathBuf;

mod runtime {
    use super::*;
    use deno_core::*;
    use deno_core::snapshot_util::*;
    use deno_runtime::deno_cache::SqliteBackedCache;
    use deno_runtime::deno_fs::StdFs;
    use deno_runtime::deno_kv::sqlite::SqliteDbHandler;
    use deno_runtime::permissions::PermissionsContainer;
    use deno_runtime::*;

    deno_core::extension!(
        grey,
        esm = [dir "src/deno/js", "40_output.js"],
        customizer = |ext: &mut deno_core::ExtensionBuilder| {
            ext.esm(vec![ExtensionFileSource {
                specifier: "ext:grey/runtime/js/99_main.js",
                code: ExtensionFileSourceCode::LoadedFromFsDuringSnapshot(
                std::path::PathBuf::from(deno_runtime::js::PATH_FOR_99_MAIN_JS),
                ),
            }]);
        }
    );

    pub fn create_deno_snapshot(snapshot_path: PathBuf) {
        let extensions = vec![
            deno_webidl::deno_webidl::init_ops(),
            deno_console::deno_console::init_ops(),
            deno_url::deno_url::init_ops(),
            deno_web::deno_web::init_ops::<PermissionsContainer>(
                deno_web::BlobStore::default(),
                Default::default(),
            ),
            deno_fetch::deno_fetch::init_ops::<PermissionsContainer>(Default::default()),
            deno_cache::deno_cache::init_ops::<SqliteBackedCache>(None),
            deno_websocket::deno_websocket::init_ops::<PermissionsContainer>(
                "".to_owned(),
                None,
                None,
            ),
            deno_webstorage::deno_webstorage::init_ops(None),
            deno_crypto::deno_crypto::init_ops(None),
            deno_broadcast_channel::deno_broadcast_channel::init_ops(
            deno_broadcast_channel::InMemoryBroadcastChannel::default(),
                false, // No --unstable.
            ),
            deno_ffi::deno_ffi::init_ops::<PermissionsContainer>(false),
            deno_net::deno_net::init_ops::<PermissionsContainer>(
                None, false, // No --unstable.
                None,
            ),
            deno_tls::deno_tls::init_ops(),
            deno_kv::deno_kv::init_ops(
            SqliteDbHandler::<PermissionsContainer>::new(None),
                false, // No --unstable.
            ),
            deno_napi::deno_napi::init_ops::<PermissionsContainer>(),
            deno_http::deno_http::init_ops(),
            deno_io::deno_io::init_ops(Default::default()),
            deno_fs::deno_fs::init_ops::<_, PermissionsContainer>(false, StdFs),
            deno_node::deno_node::init_ops::<deno_runtime::RuntimeNodeEnv>(None),
            grey::init_ops_and_esm()
        ];

        create_snapshot(CreateSnapshotOptions {
            cargo_manifest_dir: env!("CARGO_MANIFEST_DIR"),
            snapshot_path,
            startup_snapshot: Some(deno_runtime::js::deno_isolate_init()),
            extensions,
            compression_cb: None,
            snapshot_module_load_cb: None,
        })
    }
}

fn main() {
    println!("cargo:rustc-env=TARGET={}", std::env::var("TARGET").unwrap());
    println!("cargo:rustc-env=PROFILE={}", std::env::var("PROFILE").unwrap());

    let src_path = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rerun-if-changed={}", src_path.join("deno/js/40_output.js").display());

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());

    runtime::create_deno_snapshot(out_dir.join("RUNTIME_SNAPSHOT.bin"))
}