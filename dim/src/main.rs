mod runtime;

use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use dim::streaming;
use dim_core as dim;
use runtime::ApplicationContext;
use xtra::spawn::Tokio;

#[derive(Debug, clap::Parser)]
#[clap(name = "Dim", about = "Dim, a media manager fueled by dark forces.")]
#[clap(version = env!("CARGO_PKG_VERSION"), author = env!("CARGO_PKG_AUTHORS"))]
#[clap(rename_all = "kebab")]
struct Args {
    #[clap(short, long, env = "DIM_CONFIG_PATH")]
    config: Option<PathBuf>,
    /// Override the configured listener IP. Non-loopback addresses explicitly opt into LAN use.
    #[clap(long, env = "DIM_BIND_ADDRESS")]
    bind_address: Option<std::net::IpAddr>,
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Failed to register SIGTERM handler");
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if let Err(error) = result { tracing::error!(?error, "SIGINT handler failed"); }
                tracing::info!("SIGINT received, shutting down");
            }
            _ = terminate.recv() => tracing::info!("SIGTERM received, shutting down"),
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
    let config_path = args
        .config
        .unwrap_or_else(|| PathBuf::from("config/config.toml"));
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create a tokio runtime");
    if let Err(error) = runtime.block_on(run(config_path, args.bind_address)) {
        eprintln!("Dim startup failed: {error}");
        std::process::exit(1);
    }
}

async fn run(
    config_path: PathBuf,
    bind_override: Option<std::net::IpAddr>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut context = ApplicationContext::build(config_path.clone()).await?;
    if let Some(bind_address) = bind_override {
        context.settings = context.settings.with_bind_override(bind_address)?;
    }
    let global_settings = context.settings.running().clone();

    // Compatibility boundaries for scanner/artwork code not yet converted to injection. Runtime
    // ownership remains with `context`; these accessors are initialized once and never live-reload.
    dim::init_global_settings(Some(config_path.to_string_lossy().into_owned()))?;
    dim_database::set_conn(context.database.clone());
    dim_database::set_key(
        global_settings
            .secret_key
            .expect("context always provisions a key"),
    );
    let _ = dim_core::core::METADATA_PATH.set(global_settings.metadata_dir.clone());

    let _logging_guard = dim::setup_logging_at(&context.paths.logs, global_settings.verbose);
    for diagnostic in dim::diagnostics::disk_diagnostics(&context.paths)? {
        tracing::info!(path = %diagnostic.path.display(), available_bytes = diagnostic.available_bytes, "Writable-state disk diagnostic");
        if diagnostic.available_bytes < 1024 * 1024 * 1024 {
            tracing::warn!(path = %diagnostic.path.display(), available_bytes = diagnostic.available_bytes, "Less than 1 GiB is available");
        }
    }
    let reconciliation = dim::diagnostics::reconcile(&context.database, &context.paths).await?;
    if reconciliation.missing_media_files > 0 || reconciliation.missing_metadata_files > 0 {
        tracing::warn!(
            missing_media_files = reconciliation.missing_media_files,
            missing_metadata_files = reconciliation.missing_metadata_files,
            samples = ?reconciliation.samples,
            "Database/filesystem differences detected; no files or rows were changed"
        );
    }

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
        return Err("FFmpeg/FFprobe startup validation failed".into());
    }

    if let Some(limit) = fdlimit::raise_fd_limit() {
        tracing::info!(limit, "Raising fd limit");
    }
    nightfall::profiles::profiles_init(crate::streaming::FFMPEG_BIN.to_string());

    let outbox_reactor = dim::reactor::handler::EventReactor::new(context.database.clone())
        .with_websocket(context.event_tx.clone());
    let outbox_handle = tokio::spawn(
        dim::reactor::OutboxDispatcher::new(context.database.clone(), outbox_reactor)
            .run(context.shutdown_receiver()),
    );

    let stream_manager = nightfall::StateManager::new(
        &mut Tokio::Global,
        global_settings.cache_dir.clone(),
        crate::streaming::FFMPEG_BIN.to_string(),
    );
    let gc_manager = stream_manager.clone();
    let mut gc_shutdown = context.shutdown_receiver();
    let gc_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = interval.tick() => if let Err(error) = gc_manager.garbage_collect().await { tracing::warn!(?error, "Stream garbage collection failed"); },
                changed = gc_shutdown.changed() => if changed.is_err() || *gc_shutdown.borrow() { break; },
            }
        }
    });

    if !global_settings.quiet_boot {
        tracing::info!("Scanning for media files...");
        dim::core::run_scanners(
            context.database.clone(),
            context.event_tx.clone(),
            &context.library_workers,
        )
        .await;
    }

    let shutdown_tx = context.shutdown_sender();
    let signal_handle = tokio::spawn(async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });
    let mut web_shutdown = context.shutdown_receiver();
    let web_shutdown_future =
        async move { while !*web_shutdown.borrow() && web_shutdown.changed().await.is_ok() {} };
    let bind_address = global_settings.bind_address.parse()?;
    let address = std::net::SocketAddr::new(bind_address, global_settings.port);
    let deployment = if bind_address.is_loopback() {
        "local"
    } else {
        "lan-opt-in"
    };
    tracing::info!(%address, deployment, https_reverse_proxy = global_settings.https_reverse_proxy, "Launching Dim with effective listener");
    let event_rx = context.take_event_rx();
    dim_web::start_webserver(
        address,
        context.database.clone(),
        context.settings.clone(),
        context.event_tx.clone(),
        stream_manager.clone(),
        event_rx,
        context.library_workers.clone(),
        web_shutdown_future,
    )
    .await
    .map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!("HTTP listener at {address} failed: {error}"),
        )
    })?;

    context.request_shutdown();
    context.shutdown().await;
    let _ = gc_handle.await;
    if let Err(error) = stream_manager.shutdown_all().await {
        tracing::error!(?error, "Failed to stop all transcoding sessions");
    }
    let _ = outbox_handle.await;
    if !signal_handle.is_finished() {
        signal_handle.abort();
    }
    let _ = signal_handle.await;
    tracing::info!("Shutdown complete");
    Ok(())
}
