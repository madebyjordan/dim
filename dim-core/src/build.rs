use std::env;
use std::path::PathBuf;

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

fn database_url(path: &std::path::Path) -> String {
    if cfg!(windows) {
        format!("sqlite:{}", path.to_string_lossy().replace('\\', "/"))
    } else {
        format!("sqlite://{}", path.display())
    }
}

fn main() {
    let db_file = target_dir().join("dim_dev.db");
    println!("cargo:rustc-env=DATABASE_URL={}", database_url(&db_file));

    println!("cargo:rerun-if-changed=build.rs");
}
