use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, RwLock};
use tokio::time::sleep;

use crate::scene::SceneCache;

pub async fn run(
    addr: SocketAddr,
    tx: broadcast::Sender<Arc<String>>,
    last: Arc<RwLock<Option<Arc<String>>>>,
    scene: Arc<RwLock<SceneCache>>,
) {
    loop {
        match TcpStream::connect(addr).await {
            Ok(stream) => {
                eprintln!("bridge: connected to {addr}");
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => {
                            eprintln!("bridge: closed by peer");
                            break;
                        }
                        Ok(_) => {
                            let trimmed = line.trim_end();
                            if trimmed.is_empty() {
                                continue;
                            }
                            match wrap_envelope(trimmed) {
                                Ok((text, parsed)) => {
                                    *scene.write().await = SceneCache::from_envelope(&parsed);
                                    let arc = Arc::new(text);
                                    *last.write().await = Some(arc.clone());
                                    let _ = tx.send(arc);
                                }
                                Err(e) => eprintln!("bridge: bad JSON ({e}): {trimmed}"),
                            }
                        }
                        Err(e) => {
                            eprintln!("bridge: read error: {e}");
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("bridge: connect failed: {e}");
            }
        }
        sleep(Duration::from_secs(2)).await;
    }
}

fn wrap_envelope(raw: &str) -> Result<(String, serde_json::Value), serde_json::Error> {
    let mut v: serde_json::Value = serde_json::from_str(raw)?;
    if let Some(obj) = v.as_object_mut() {
        obj.insert("op".into(), serde_json::Value::String("snapshot".into()));
        obj.insert("v".into(), serde_json::Value::from(1));
    }
    let text = v.to_string();
    Ok((text, v))
}
