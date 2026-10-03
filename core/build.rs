use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../proto/daemon.proto");
    tonic_prost_build::compile_protos("../proto/daemon.proto")?;

    if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
        let runner_dir = PathBuf::from(&manifest_dir).join("../startup");
        let profile = env::var("PROFILE").unwrap_or_else(|_| "release".to_string());
        let (cargo_profile, out_profile) = match profile.as_str() {
            "debug" | "dev" => ("dev", "debug"),
            _ => (profile.as_str(), profile.as_str()),
        };
        let target = env::var("TARGET").unwrap_or_default();

        // Persistent target dir shared across check/build invocations so the
        // launcher is built once, not once per OUT_DIR hash.
        let startup_target_dir = persistent_startup_target_dir();
        let exe_path = startup_exe_path(&startup_target_dir, &target, out_profile);

        // Fast path: reuse a fresh launcher instead of spawning nested cargo.
        // Same sources + same profile => identical bytes embedded below.
        if exe_up_to_date(&exe_path, &runner_dir)
            || env::var("TAURINE_SKIP_STARTUP_BUILD").as_deref() == Ok("1") && exe_path.exists()
        {
            println!("cargo:rustc-env=STARTUP_RUNNER_PATH={}", exe_path.display());
        } else {
            let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
            let mut cmd = Command::new(&cargo);
            cmd.arg("build")
                .arg("--profile")
                .arg(cargo_profile)
                .arg("--target-dir")
                .arg(&startup_target_dir);
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
            println!("cargo:rustc-env=STARTUP_RUNNER_PATH={}", exe_path.display());
        }

        println!("cargo:rerun-if-changed=../startup/src");
        println!("cargo:rerun-if-changed=../startup/Cargo.toml");
        println!("cargo:rerun-if-changed=../startup/build.rs");
    }

    Ok(())
}

/// `<target>/startup-target`: OUT_DIR is `<target>/<profile>/build/<pkg>-<hash>/out`,
/// so four ancestors up is `<target>`. Falls back to the old OUT_DIR-scoped dir
/// if the layout is unexpected (output is identical either way).
fn persistent_startup_target_dir() -> PathBuf {
    if let Ok(out_dir) = env::var("OUT_DIR") {
        let mut target = PathBuf::from(&out_dir);
        for _ in 0..4 {
            if !target.pop() {
                break;
            }
        }
        if target.file_name().is_some() {
            return target.join("startup-target");
        }
    }
    PathBuf::from(env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string()))
        .join("startup-target")
}

fn startup_exe_path(base: &Path, target: &str, out_profile: &str) -> PathBuf {
    if target.is_empty() {
        base.join(out_profile).join("taurine-startup.exe")
    } else {
        base.join(target)
            .join(out_profile)
            .join("taurine-startup.exe")
    }
}

/// True when the cached launcher exists and is newer than every input that
/// affects it. Timestamps are the trigger; content equality is unnecessary
/// because a stale exe would only come from an older source tree.
fn exe_up_to_date(exe: &Path, runner_dir: &Path) -> bool {
    let Ok(exe_meta) = std::fs::metadata(exe) else {
        return false;
    };
    let Ok(exe_mtime) = exe_meta.modified() else {
        return false;
    };
    // honey: mtime comparison only; content hash would re-read the tree every build.
    for rel in ["src", "Cargo.toml", "build.rs"] {
        let path = runner_dir.join(rel);
        if !path_newer_than(&path, exe_mtime) {
            return false;
        }
    }
    true
}

fn path_newer_than(path: &Path, than: std::time::SystemTime) -> bool {
    if path.is_file() {
        return file_newer_than(path, than);
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if !path_newer_than(&p, than) {
                return false;
            }
        } else if !file_newer_than(&p, than) {
            return false;
        }
    }
    true
}

fn file_newer_than(path: &Path, than: std::time::SystemTime) -> bool {
    // Missing inputs fail closed: rebuild instead of embedding a stale launcher.
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(mtime) = meta.modified() else {
        return false;
    };
    mtime <= than
}
