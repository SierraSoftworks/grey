use std::process::Command;

fn main() {
    if std::env::var("PROFILE").unwrap_or_default() == "debug" {
        println!("cargo:rerun-if-changed=api/");
        println!("cargo:rerun-if-changed=ui/");
        println!("cargo:rerun-if-changed=Trunk.toml");

        println!("cargo:info=Building UI assets with trunk");

        // Build the UI with trunk using a clean environment
        let mut cmd = Command::new("trunk");
        cmd.arg("build")
            .arg("--release")
            .env_remove("CARGO_ENCODED_RUSTFLAGS ")
            .env_remove("CARGO_BUILD_RUSTFLAGS ")
            .env_remove("RUSTFLAGS");  // Keep manifest dir

        let output = cmd.output()
            .expect("Failed to execute trunk build. Make sure trunk is installed: cargo install trunk");

        if !output.status.success() {
            panic!(
                "trunk build failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        println!("cargo:info=Successfully built UI assets with trunk");

    }
}
