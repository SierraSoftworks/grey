use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=api/");
    println!("cargo:rerun-if-changed=ui/");
    println!("cargo:rerun-if-changed=Trunk.toml");

    println!("cargo:info=Building UI assets with trunk");

    // Build the UI with trunk using a clean environment
    let mut cmd = Command::new("trunk");
    cmd.arg("build")
        .arg("--release")
        .env_clear()  // Clear all environment variables
        .env("PATH", std::env::var("PATH").unwrap_or_default())  // Keep PATH for finding binaries
        .env("CARGO_MANIFEST_DIR", std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());  // Keep manifest dir

    // Add common environment variables if they exist
    for env_var in &["HOME", "USER", "USERPROFILE", "APPDATA", "TEMP", "TMP", "CARGO_HOME", "RUSTUP_HOME"] {
        if let Ok(value) = std::env::var(env_var) {
            cmd.env(env_var, value);
        }
    }

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
