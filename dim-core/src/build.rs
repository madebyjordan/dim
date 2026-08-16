use std::env;
use std::path::{Path, PathBuf};

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

fn main() {
    let db_file = target_dir().join("dim_dev.db");
    println!(
        "cargo:rustc-env=DATABASE_URL=sqlite://{}",
        db_file.display()
    );

    if Path::new("../ui/build").exists() {
        println!("cargo:rustc-cfg=feature=\"embed_ui\"");
    } else {
        println!("cargo:warning=`ui/build` does not exist.");
        println!(
            "cargo:warning=If you wish to embed the webui, run `corepack yarn build` in `ui`."
        );
    }

    println!("cargo:rerun-if-changed=../ui/build");
    println!("cargo:rerun-if-changed=build.rs");
}
