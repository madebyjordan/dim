use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use dim::streaming;
use xtra::spawn::Tokio;

use dim_core as dim;
#[derive(Debug, clap::Parser)]
#[clap(name = "Dim", about = "Dim, a media manager fueled by dark forces.")]
#[clap(version = env!("CARGO_PKG_VERSION"), author = env!("CARGO_PKG_AUTHORS"))]
#[clap(rename_all = "kebab")]
struct Args {
    #[clap(short, long, env = "DIM_CONFIG_PATH")]
    config: Option<PathBuf>,
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Failed to register SIGTERM handler");
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if let Err(error) = result {
                    tracing::error!(?error, "SIGINT handler failed");
                }
                tracing::info!("SIGINT received, shutting down");
            }
            _ = terminate.recv() => {
                tracing::info!("SIGTERM received, shutting down");
            }
        }
    }

    #[cfg(not(unix))]
    {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(?error, "Shutdown signal handler failed");
        }
        tracing::info!("Shutdown signal received, shutting down");
    }
}

fn main() {
    let args = Args::parse();
    let _ = std::fs::create_dir_all(dim::utils::ffpath("config"));

    let config_path = args
        .config
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or(dim::utils::ffpath("config/config.toml"));

    // initialize global settings.
    dim::init_global_settings(Some(config_path)).expect("Failed to initialize global settings.");

    let global_settings = dim::get_global_settings();

    // never panics because we set a default value to metadata_dir
    let _ = std::fs::create_dir_all(global_settings.metadata_dir.clone());

    // set our jwt secret key
    let settings_clone = global_settings.clone();
    let secret_key = global_settings.secret_key.unwrap_or_else(move || {
        let secret_key = dim_database::generate_key();
        dim::set_global_settings(dim::GlobalSettings {
            secret_key: Some(secret_key),
            ..settings_clone
        })
        .expect("Failed to save JWT secret_key.");
        secret_key
    });

    dim_database::set_key(secret_key);

    dim_core::core::METADATA_PATH
        .set(global_settings.metadata_dir.clone())
        .expect("Failed to set METADATA_PATH");

    // Keep the nonblocking logging worker alive until all shutdown cleanup has completed. Dropping
    // this guard flushes buffered log records.
    let _logging_guard = dim::setup_logging(global_settings.verbose);

    {
        let failed = streaming::ffcheck()
            .into_iter()
            .fold(false, |failed, item| match item {
                Ok(stdout) => {
                    tracing::info!("{}", stdout);
                    failed
                }

                Err(program) => {
                    tracing::error!("Could not find: {}", program);
                    true
                }
            });

        if failed {
            drop(_logging_guard);
            std::process::exit(1);
        }
    }

    // The mediafile scanner is super hungry for fds. Increase our limits here as much as possible.
    if let Some(limit) = fdlimit::raise_fd_limit() {
        tracing::info!(limit, "Raising fd limit.");
    }

    nightfall::profiles::profiles_init(crate::streaming::FFMPEG_BIN.to_string());

    let async_main = async move {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let pool = dim_database::get_conn().await.unwrap();
        let library_workers = dim::workers::LibraryWorkers::default();

        // Before we start making DB-calls we need to initialize our CDC pipeline.
        let reactor_handle = {
            let mut lock = pool.writer().lock_owned().await;
            let mut reactor_core = dim::reactor::ReactorCore::new();
            reactor_core.register(&mut lock).await;

            let reactor = dim::reactor::handler::EventReactor::new(pool.clone())
                .with_websocket(event_tx.clone());

            tokio::spawn(reactor_core.react(reactor))
        };

        let stream_manager = nightfall::StateManager::new(
            &mut Tokio::Global,
            global_settings.cache_dir.clone(),
            crate::streaming::FFMPEG_BIN.to_string(),
        );

        let stream_manager_clone = stream_manager.clone();

        // GC the stream manager every second.
        let gc_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(1000));
            interval.tick().await;

            loop {
                interval.tick().await;
                let _ = stream_manager_clone.garbage_collect().await.unwrap();
            }
        });

        if !global_settings.quiet_boot {
            tracing::info!("Scanning for media files...");
            dim::core::run_scanners(event_tx.clone(), &library_workers).await;
        }

        tracing::info!("Launching Dim");

        let address = std::net::SocketAddr::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)),
            global_settings.port,
        );

        dim_web::start_webserver(
            address,
            event_tx,
            stream_manager.clone(),
            event_rx,
            library_workers.clone(),
            shutdown_signal(),
        )
        .await;

        library_workers.shutdown().await;

        gc_handle.abort();
        let _ = gc_handle.await;

        if let Err(error) = stream_manager.shutdown_all().await {
            tracing::error!(?error, "Failed to stop all transcoding sessions");
        }

        reactor_handle.abort();
        let _ = reactor_handle.await;
        tracing::info!("Shutdown complete");
    };

    tokio::runtime::Runtime::new()
        .expect("Failed to create a tokio runtime.")
        .block_on(async_main);
}
