use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

fn target_dir() -> PathBuf {
    let workspace_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .to_owned();

    match env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        Some(path) if path.is_absolute() => path,
        Some(path) => workspace_dir.join(path),
        None => workspace_dir.join("target"),
    }
}

fn git_value(args: &[&str], fallback: &str) -> String {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

fn main() -> Result<(), Box<dyn Error>> {
    // Both values affect the absolute database path exported to SQLx. Tracking them prevents
    // Cargo from reusing stale build-script output when a checkout or target directory moves.
    println!("cargo:rerun-if-env-changed=CARGO_MANIFEST_DIR");
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_DIR");

    let db_file = target_dir().join("dim_dev.db");
    println!(
        "cargo:rustc-env=DATABASE_URL=sqlite://{}",
        db_file.display()
    );

    println!(
        "cargo:rustc-env=GIT_TAG={}",
        git_value(&["describe", "--abbrev=0"], "untagged")
    );
    println!(
        "cargo:rustc-env=GIT_SHA_256={}",
        git_value(&["rev-parse", "HEAD"], "unknown")
    );

    if Path::new("../eclipse/build").exists() {
        println!("cargo:rustc-cfg=feature=\"embed_ui\"");
    } else {
        println!("cargo:warning=`eclipse/build` does not exist.");
        println!("cargo:warning=To embed Eclipse, run `corepack pnpm --dir eclipse build`.");
    }

    println!("cargo:rerun-if-changed=../eclipse/build");
    println!("cargo:rerun-if-changed=build.rs");

    Ok(())
}
