use std::env;
use std::error::Error;
use std::fs;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let workspace_dir = std::path::Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let mut out_dir = env::var("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| workspace_dir.join("target"));
    if out_dir.is_relative() {
        out_dir = std::path::absolute(out_dir)?;
    }

    let db_file = out_dir.join("dim_dev.db").display().to_string();
    println!("cargo:rustc-env=DATABASE_URL=sqlite://{db_file}");
    println!(
        "cargo:warning=Generating {:?} from latest migrations.",
        db_file
    );

    let _ = fs::remove_file(&db_file);

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::from_str(db_file.as_ref())?.create_if_missing(true),
        )
        .await?;

    sqlx::migrate!().run(&pool).await.map_err(|e| {
        println!("cargo:error=Migration failed: {:?}", e);
        e
    })?;

    println!("cargo:warning=Built database {}.", db_file);

    println!("cargo:rerun-if-changed=database/src/build.rs");
    println!("cargo:rerun-if-changed=database/migrations");

    Ok(())
}
