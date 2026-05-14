use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::{broadcast, RwLock};

use host_input_helper::injector::{Injector, LogInjector};
use host_input_helper::scene::SceneCache;
use host_input_helper::{bridge, server};

const DEFAULT_BRIDGE: &str = "127.0.0.1:54321";
const DEFAULT_WS_BIND: &str = "127.0.0.1:54323";
const SNAPSHOT_CAP: usize = 8;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bridge_addr: SocketAddr = env::var("BRIDGE_ADDR")
        .unwrap_or_else(|_| DEFAULT_BRIDGE.into())
        .parse()?;
    let ws_bind: SocketAddr = env::var("WS_BIND")
        .unwrap_or_else(|_| DEFAULT_WS_BIND.into())
        .parse()?;

    let injector = make_injector();
    eprintln!(
        "host-input-helper: bridge={bridge_addr} ws={ws_bind} injector={}",
        injector_name()
    );

    let (snap_tx, _) = broadcast::channel::<Arc<String>>(SNAPSHOT_CAP);
    let last: Arc<RwLock<Option<Arc<String>>>> = Arc::new(RwLock::new(None));
    let scene: Arc<RwLock<SceneCache>> = Arc::new(RwLock::new(SceneCache::default()));

    tokio::spawn(bridge::run(
        bridge_addr,
        snap_tx.clone(),
        last.clone(),
        scene.clone(),
    ));

    let listener = TcpListener::bind(ws_bind).await?;
    eprintln!("listening for WS clients on {ws_bind}");

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, peer)) => {
                        let rx = snap_tx.subscribe();
                        let inj = injector.clone();
                        let last = last.clone();
                        let scene = scene.clone();
                        tokio::spawn(async move {
                            if let Err(e) = server::client_session(stream, peer, rx, inj, last, scene).await {
                                eprintln!("client {peer} ended: {e}");
                            }
                        });
                    }
                    Err(e) => eprintln!("accept error: {e}"),
                }
            }
            _ = signal::ctrl_c() => {
                eprintln!("shutting down");
                return Ok(());
            }
        }
    }
}

fn make_injector() -> Arc<dyn Injector> {
    let pref = env::var("INJECTOR").ok().unwrap_or_default();
    let pref = pref.to_ascii_lowercase();
    match pref.as_str() {
        "log" => Arc::new(LogInjector),
        "enigo" => build_enigo_or_warn(),
        "" => default_injector(),
        other => {
            eprintln!("INJECTOR={other:?} not recognized; using default");
            default_injector()
        }
    }
}

#[cfg(target_os = "windows")]
fn default_injector() -> Arc<dyn Injector> {
    Arc::new(host_input_helper::enigo_injector::EnigoInjector::new())
}

#[cfg(not(target_os = "windows"))]
fn default_injector() -> Arc<dyn Injector> {
    Arc::new(LogInjector)
}

#[cfg(target_os = "windows")]
fn build_enigo_or_warn() -> Arc<dyn Injector> {
    Arc::new(host_input_helper::enigo_injector::EnigoInjector::new())
}

#[cfg(not(target_os = "windows"))]
fn build_enigo_or_warn() -> Arc<dyn Injector> {
    eprintln!("INJECTOR=enigo requested but not built on this platform; using log");
    Arc::new(LogInjector)
}

#[cfg(target_os = "windows")]
fn injector_name() -> &'static str {
    if env::var("INJECTOR").as_deref() == Ok("log") { "log" } else { "enigo" }
}

#[cfg(not(target_os = "windows"))]
fn injector_name() -> &'static str {
    "log"
}
