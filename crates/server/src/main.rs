//! EduGuard server entrypoint.

mod assets;
mod config;
mod db;
mod keywords;
mod routes;
mod state;
mod ws;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use state::AppState;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "eduguard-server", version, about = "EduGuard management server")]
struct Cli {
    /// Path to server.toml
    #[arg(long, global = true)]
    config: Option<std::path::PathBuf>,

    /// Override listen address (e.g. 0.0.0.0:9000)
    #[arg(long, global = true)]
    listen: Option<String>,

    /// Override data directory
    #[arg(long, global = true)]
    data_dir: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the server (default).
    Serve,
    /// Print configuration paths and exit.
    Info,
    /// Create a new enrollment token and print it.
    NewEnrollmentToken {
        #[arg(long, default_value = "")]
        label: String,
    },
    /// Reset the admin password (prints a fresh one).
    ResetAdminPassword,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,axum=warn".into()),
        )
        .init();

    let cli = Cli::parse();
    let config_path = cli.config.clone().unwrap_or_else(Config::default_path);
    let mut cfg = Config::load(&config_path)
        .with_context(|| format!("loading config from {}", config_path.display()))?;
    if let Some(l) = cli.listen {
        cfg.listen = l;
    }
    if let Some(d) = cli.data_dir {
        cfg.data_dir = d;
    }

    match cli.command.unwrap_or(Cmd::Serve) {
        Cmd::Info => {
            println!("config:  {}", config_path.display());
            println!("data:    {}", cfg.data_dir.display());
            println!("listen:  {}", cfg.listen);
            println!("tls:     {}", cfg.tls_enabled());
        }
        Cmd::NewEnrollmentToken { label } => {
            let database = db::Db::open(&cfg.data_dir.join("eduguard.db"))?;
            let token = format!("eg-{}", config::random_token(16));
            database.add_enrollment_token(&token, &label, None)?;
            println!("{token}");
        }
        Cmd::ResetAdminPassword => {
            let _database = db::Db::open(&cfg.data_dir.join("eduguard.db"))?;
            let pw = config::random_token(12);
            cfg.admin_password = pw.clone();
            let text = toml::to_string_pretty(&cfg)?;
            std::fs::write(&config_path, text)
                .with_context(|| format!("writing {}", config_path.display()))?;
            println!("{pw}");
        }
        Cmd::Serve => serve(cfg, config_path)?,
    }
    Ok(())
}

fn serve(mut cfg: Config, config_path: std::path::PathBuf) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    // Bootstrap secrets before blocking on runtime.
    let database = db::Db::open(&cfg.data_dir.join("eduguard.db"))?;
    let mut bootstrap_notes: Vec<String> = Vec::new();

    if cfg.admin_password.is_empty() {
        cfg.admin_password = config::random_token(12);
        let text = toml::to_string_pretty(&cfg)?;
        std::fs::write(&config_path, text)
            .with_context(|| format!("writing {}", config_path.display()))?;
        bootstrap_notes.push(format!("admin password: {}", cfg.admin_password));
    }
    if database.get_setting("policy_template")?.is_none() {
        let mut policy = protocol::Policy::default();
        policy.school_name = cfg.school_name.clone();
        database.set_setting(
            "policy_template",
            &serde_json::to_string_pretty(&policy)?,
        )?;
    }
    if database.get_setting("keywords")?.is_none() {
        database.set_setting("keywords", &keywords::seed_json())?;
    }
    let have_token = !database.list_enrollment_tokens()?.is_empty();
    let default_token = if !have_token {
        let token = format!("eg-{}", config::random_token(16));
        database.add_enrollment_token(&token, "default", None)?;
        bootstrap_notes.push(format!("enrollment token: {token}"));
        Some(token)
    } else {
        None
    };
    let _ = default_token;

    for note in &bootstrap_notes {
        tracing::info!(secret = %note, "first-run");
        println!("[eduguard] first run -> {note}");
    }

    let state = AppState::new(database, cfg.clone());

    // Background: retention + session pruning.
    {
        let state = state.clone();
        rt.spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(3600));
            loop {
                ticker.tick().await;
                if let Err(e) = run_retention(&state) {
                    tracing::warn!(error = %e, "retention task");
                }
            }
        });
    }

    // Background: relay console "viewing" state to agents so thumbnail FPS can
    // adapt (B1 bandwidth adaptivity).
    {
        let state = state.clone();
        rt.spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5));
            let mut last = false;
            loop {
                ticker.tick().await;
                let last_view = state.last_view.load(Ordering::Relaxed);
                let active = chrono::Utc::now().timestamp() - last_view < 10;
                if active != last {
                    last = active;
                    state
                        .broadcast_devices(protocol::ServerMessage::Viewing { active })
                        .await;
                    state.viewing.store(active, Ordering::Relaxed);
                }
            }
        });
    }

    let app = axum::Router::new()
        .merge(routes::router())
        .route("/ws/device", axum::routing::get(ws::device_ws))
        .route("/ws/console", axum::routing::get(ws::console_ws))
        .route("/", axum::routing::get(|| async { assets::index() }))
        .route("/app.js", axum::routing::get(|| async { assets::app_js() }))
        .route("/style.css", axum::routing::get(|| async { assets::style_css() }))
        .route("/favicon.svg", axum::routing::get(|| async { assets::favicon() }))
        .with_state(state.clone());

    let addr: std::net::SocketAddr = cfg
        .listen
        .parse()
        .with_context(|| format!("bad listen address {}", cfg.listen))?;

    if cfg.tls_enabled() {
        tracing::info!(%addr, "eduguard-server listening (https)");
        rt.block_on(async move {
            let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(
                cfg.tls_cert.clone().unwrap(),
                cfg.tls_key.clone().unwrap(),
            )
            .await
            .context("loading TLS certificate/key")?;
            let handle = axum_server::Handle::new();
            let handle2 = handle.clone();
            tokio::spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                tracing::info!("shutting down");
                handle2.graceful_shutdown(Some(std::time::Duration::from_secs(5)));
            });
            axum_server::bind_rustls(addr, tls)
                .handle(handle)
                .serve(app.into_make_service())
                .await?;
            Ok::<(), anyhow::Error>(())
        })?;
    } else {
        tracing::info!(%addr, "eduguard-server listening (http)");
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .with_context(|| format!("binding {addr}"))?;
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                    tracing::info!("shutting down");
                })
                .await?;
            Ok::<(), anyhow::Error>(())
        })?;
    }
    Ok(())
}

fn run_retention(state: &Arc<state::AppState>) -> Result<()> {
    let db = &state.db;
    let days = state.config.screenshot_retention_days;
    let cutoff = chrono::Utc::now().timestamp() - (days * 86400) as i64;
    let shot_dir = state.config.data_dir.join("screenshots");
    if let Ok(devs) = std::fs::read_dir(&shot_dir) {
        for dev in devs.flatten() {
            if let Ok(files) = std::fs::read_dir(dev.path()) {
                for f in files.flatten() {
                    if let Some(stem) = f.file_name().to_string_lossy().strip_suffix(".jpg") {
                        if let Ok(ts) = stem.parse::<i64>() {
                            if ts < cutoff {
                                let _ = std::fs::remove_file(f.path());
                            }
                        }
                    }
                }
            }
        }
    }
    db.prune_events(state.config.event_retention_days)?;
    db.prune_commands(state.config.event_retention_days)?;
    db.prune_sessions()?;
    tracing::info!("retention pass complete");
    Ok(())
}
