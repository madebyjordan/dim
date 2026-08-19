use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Both values affect the absolute database path exported to SQLx. Tracking them prevents
    // Cargo from reusing stale build-script output when a checkout or target directory moves.
    println!("cargo:rerun-if-env-changed=CARGO_MANIFEST_DIR");
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_DIR");

    let target_dir = target_dir();
    fs::create_dir_all(&target_dir)?;
    let db_file = target_dir.join("dim_dev.db");
    println!("cargo:rustc-env=DATABASE_URL={}", database_url(&db_file));
    println!(
        "cargo:warning=Generating {:?} from latest migrations.",
        db_file
    );

    let _ = fs::remove_file(&db_file);

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::from_str(db_file.to_str().unwrap())?
                .create_if_missing(true),
        )
        .await?;

    // Load migrations at build-script runtime. Embedding them with `migrate!` here leaves Cargo
    // able to rerun a stale build-script binary after a migration file changes.
    sqlx::migrate::Migrator::new(PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("migrations"))
        .await?
        .run(&pool)
        .await
        .map_err(|e| {
            println!("cargo:error=Migration failed: {:?}", e);
            e
        })?;

    println!("cargo:warning=Built database {}.", db_file.display());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=migrations");

    Ok(())
}
