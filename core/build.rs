use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../proto/daemon.proto");
    tonic_prost_build::compile_protos("../proto/daemon.proto")?;

    if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let out_dir = env::var("OUT_DIR").unwrap();
        let target = env::var("TARGET").unwrap_or_default();
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let runner_dir = PathBuf::from(manifest_dir).join("../startup");

        // Embed a launcher matching our own profile: debug builds get a debug
        // launcher (DEV env overrides work), release gets release (fixed paths).
        // NOTE: Cargo sets PROFILE to "debug"/"release" (legacy names), but the
        // profile flag expects "dev"/"release" — "debug" is reserved.
        let profile = env::var("PROFILE").unwrap_or_else(|_| "release".to_string());
        let (cargo_profile, out_profile) = match profile.as_str() {
            "debug" | "dev" => ("dev", "debug"),
            _ => (profile.as_str(), profile.as_str()),
        };

        let mut cmd = Command::new(&cargo);
        cmd.arg("build")
            .arg("--profile")
            .arg(cargo_profile)
            .arg("--target-dir")
            .arg(format!("{}/startup-target", out_dir));

        if !target.is_empty() {
            cmd.arg("--target").arg(&target);
        }

        let status = cmd
            .current_dir(&runner_dir)
            .status()
            .expect("Failed to build startup runner");

        if !status.success() {
            panic!("Startup runner failed to build with status: {}", status);
        }

        let exe_path = if target.is_empty() {
            format!(
                "{}/startup-target/{}/taurine-startup.exe",
                out_dir, out_profile
            )
        } else {
            format!(
                "{}/startup-target/{}/{}/taurine-startup.exe",
                out_dir, target, out_profile
            )
        };

        // Let Windows code know exactly where it is so it can include_bytes! it
        println!("cargo:rustc-env=STARTUP_RUNNER_PATH={}", exe_path);

        println!("cargo:rerun-if-changed=../startup/src");
        println!("cargo:rerun-if-changed=../startup/Cargo.toml");
        println!("cargo:rerun-if-changed=../startup/build.rs");
    }

    Ok(())
}
