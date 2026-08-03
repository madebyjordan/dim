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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let target_dir = target_dir();
    fs::create_dir_all(&target_dir)?;
    let db_file = target_dir.join("dim_dev.db");
    println!(
        "cargo:rustc-env=DATABASE_URL=sqlite://{}",
        db_file.display()
    );
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

    sqlx::migrate!().run(&pool).await.map_err(|e| {
        println!("cargo:error=Migration failed: {:?}", e);
        e
    })?;

    println!("cargo:warning=Built database {}.", db_file.display());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=migrations");

    Ok(())
}
