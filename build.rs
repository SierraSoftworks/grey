use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=api/");
    println!("cargo:rerun-if-changed=ui/");
    println!("cargo:rerun-if-changed=Trunk.toml");
    
    // Build the UI with trunk
    let output = Command::new("trunk")
        .arg("build")
        .arg("--release")
        .output()
        .expect("Failed to execute trunk build. Make sure trunk is installed: cargo install trunk");

    if !output.status.success() {
        panic!(
            "trunk build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    println!("cargo:warning=Successfully built UI assets with trunk");
}
