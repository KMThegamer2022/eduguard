//! EduGuard device agent: enrollment + enforcement daemon for Windows, macOS
//! and Linux.

mod config;
mod inventory;
mod net;
mod platform;
mod scan;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use config::{AgentConfig, Identity, Paths};
use platform::overlay;

#[derive(Parser)]
#[command(name = "eduguard-agent", version, about = "EduGuard device agent")]
struct Cli {
    /// Path to agent.toml (default per-OS location)
    #[arg(long, global = true)]
    config: Option<std::path::PathBuf>,

    /// Path to identity.json (default per-OS location)
    #[arg(long, global = true)]
    identity: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the agent (default; normally started by the OS service).
    Run,
    /// Enroll this device with the server and store its identity.
    Enroll {
        /// Server base URL, e.g. http://192.168.1.10:8443
        #[arg(long)]
        server: String,
        /// Enrollment token from the console (Settings → Enrollment).
        #[arg(long)]
        token: String,
        /// Optional friendly device name.
        #[arg(long)]
        name: Option<String>,
    },
    /// Print enrollment status.
    Status,
    /// Ask for help (student "raise hand").
    Raise {
        #[arg(long)]
        note: Option<String>,
    },
    /// Send a message to staff (student chat).
    Chat { text: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = cli_paths(&cli);

    let cfg = AgentConfig::load(&paths.config).unwrap_or_default();
    let filter = if cfg.log.is_empty() {
        "info".to_string()
    } else {
        cfg.log.clone()
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| filter.into()),
        )
        .init();

    match cli.command.unwrap_or(Cmd::Run) {
        Cmd::Run => run_agent(&paths),
        Cmd::Enroll {
            server,
            token,
            name,
        } => enroll(&paths, &server, &token, name),
        Cmd::Status => status(&paths),
        Cmd::Raise { note } => {
            let identity = Identity::load(&paths.identity)?;
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(net::send_one_shot(&identity, net::new_raise_hand(note)))?;
            println!("help request sent");
            Ok(())
        }
        Cmd::Chat { text } => {
            let identity = Identity::load(&paths.identity)?;
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(net::send_one_shot(&identity, net::new_chat(text)))?;
            println!("message sent to staff");
            Ok(())
        }
    }
}

fn cli_paths(cli: &Cli) -> Paths {
    let mut p = config::default_paths();
    if let Some(c) = &cli.config {
        p.config = c.clone();
    }
    if let Some(i) = &cli.identity {
        p.identity = i.clone();
    }
    p
}

fn enroll(paths: &Paths, server: &str, token: &str, name: Option<String>) -> Result<()> {
    let server = server.trim_end_matches('/').to_string();
    let hostname = platform::hostname();
    let os = std::env::consts::OS.to_string();
    let os_version = os_version_str();
    let req = protocol::EnrollmentRequest {
        enrollment_token: token.to_string(),
        device_name: name,
        hostname: hostname.clone(),
        os: os.clone(),
        os_version,
        arch: std::env::consts::ARCH.to_string(),
        agent_version: protocol::AGENT_VERSION.to_string(),
        enrolled_by: Some(platform::active_user()),
    };
    let url = format!("{server}/api/enroll");
    let body = serde_json::to_string(&req)?;
    let resp = ureq::post(&url)
        .header("content-type", "application/json")
        .send(body.as_bytes())
        .with_context(|| format!("POST {url}"))?;
    if resp.status() != 200 {
        bail!("enrollment rejected (HTTP {})", resp.status());
    }
    let text = resp.into_body().read_to_string()?;
    let parsed: protocol::EnrollmentResponse = serde_json::from_str(&text)?;
    let identity = Identity {
        device_id: parsed.device_id.clone(),
        device_token: parsed.device_token,
        server: server.clone(),
    };
    identity.save(&paths.identity)?;

    // Persist server + name into the config for the service.
    let mut cfg = AgentConfig::load(&paths.config).unwrap_or_default();
    cfg.server = server;
    if let Some(n) = req.device_name.clone() {
        cfg.name = n;
    }
    cfg.save(&paths.config)?;

    println!("enrolled as {} (server: {})", parsed.device_id, parsed.server_version);
    println!("identity: {}", paths.identity.display());
    Ok(())
}

fn status(paths: &Paths) -> Result<()> {
    println!("config:   {} ({})", paths.config.display(), if paths.config.exists() { "present" } else { "missing" });
    println!("identity: {}", if paths.identity.exists() { paths.identity.display().to_string() } else { "not enrolled".into() });
    if paths.identity.exists() {
        let id = Identity::load(&paths.identity)?;
        println!("device:   {}", id.device_id);
        println!("server:   {}", id.server);
    }
    Ok(())
}

fn os_version_str() -> String {
    sysinfo::System::os_version().unwrap_or_else(|| "unknown".into())
}

fn run_agent(paths: &Paths) -> Result<()> {
    let cfg = AgentConfig::load(&paths.config)?;
    let identity = Identity::load(&paths.identity)?;
    if !cfg.server.is_empty() && cfg.server.trim_end_matches('/') != identity.server.trim_end_matches('/') {
        tracing::warn!("config server differs from identity server; using identity");
    }

    // Shutdown signaling (service stop).
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let shutdown = shutdown.clone();
        std::thread::spawn(move || {
            wait_for_stop_signal();
            tracing::info!("stop signal received");
            shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
            overlay::send(overlay::Update::Quit);
        });
    }

    #[cfg(target_os = "macos")]
    {
        // AppKit must own the process main thread; the network runs alongside.
        let (tx, rx) = std::sync::mpsc::channel();
        overlay::install(tx);
        let downloads = paths.downloads.clone();
        let agent = net::Agent::new(identity, downloads);
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(agent.run());
        });
        overlay::run_current_thread(rx);
        std::process::exit(0);
    }

    #[cfg(not(target_os = "macos"))]
    {
        overlay::ensure_started();
        let downloads = paths.downloads.clone();
        let agent = net::Agent::new(identity, downloads);
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        rt.block_on(async move {
            let run = tokio::spawn(async move { agent.run().await });
            loop {
                if shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                    overlay::send(overlay::Update::Quit);
                    std::process::exit(0);
                }
                if run.is_finished() {
                    std::process::exit(0);
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        });
        Ok(())
    }
}

fn wait_for_stop_signal() {
    #[cfg(unix)]
    {
        use std::sync::mpsc;
        let (tx, rx) = mpsc::channel();
        ctrlc_ish(tx);
        let _ = rx.recv();
    }
    #[cfg(windows)]
    {
        // Service Control Manager sends CTRL_BREAK/CTRL_CLOSE to services.
        std::thread::sleep(std::time::Duration::from_secs(100_000));
    }
}

#[cfg(unix)]
fn ctrlc_ish(tx: std::sync::mpsc::Sender<()>) {
    std::thread::spawn(move || {
        let mut sigint = signal_hook::iterator::Signals::new([libc::SIGTERM, libc::SIGINT])
            .expect("install signal handler");
        for _ in sigint.forever() {
            let _ = tx.send(());
            return;
        }
    });
}
