use std::env;
use std::path::Path;

fn main() {
    let workspace_dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let mut out_dir = env::var("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| workspace_dir.join("target"));
    if out_dir.is_relative() {
        out_dir = std::path::absolute(out_dir).unwrap();
    }
    let db_file = out_dir.join("dim_dev.db").display().to_string();
    println!("cargo:rustc-env=DATABASE_URL=sqlite://{db_file}");

    if Path::new("../ui/build").exists() {
        println!("cargo:rustc-cfg=feature=\"embed_ui\"");
    } else {
        println!("cargo:warning=`ui/build` does not exist.");
        println!("cargo:warning=If you wish to embed the webui, run `yarn build` in `ui`.");
    }

    println!("cargo:rerun-if-changed=ui/build");
    println!("cargo:rerun-if-changed=build.rs");
}
