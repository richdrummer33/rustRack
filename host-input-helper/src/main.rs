use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::{broadcast, RwLock};

mod bridge;
mod injector;
mod server;

use injector::{Injector, LogInjector};

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

    eprintln!("host-input-helper: bridge={bridge_addr} ws={ws_bind}");

    let (snap_tx, _) = broadcast::channel::<Arc<String>>(SNAPSHOT_CAP);
    let last: Arc<RwLock<Option<Arc<String>>>> = Arc::new(RwLock::new(None));
    let injector: Arc<dyn Injector> = Arc::new(LogInjector);

    tokio::spawn(bridge::run(bridge_addr, snap_tx.clone(), last.clone()));

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
                        tokio::spawn(async move {
                            if let Err(e) = server::client_session(stream, peer, rx, inj, last).await {
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
