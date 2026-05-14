use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};
use tokio::time::sleep;

use crate::scene::SceneCache;

pub struct BridgeCommand {
    pub module_id: i64,
    pub action: String,
    pub reply: oneshot::Sender<Result<(), String>>,
}

#[derive(Clone)]
pub struct BridgeHandle {
    cmd_tx: mpsc::Sender<BridgeCommand>,
}

impl BridgeHandle {
    pub async fn send_action(&self, module_id: i64, action: String) -> Result<(), String> {
        let (reply, rx) = oneshot::channel();
        let cmd = BridgeCommand { module_id, action, reply };
        if self.cmd_tx.send(cmd).await.is_err() {
            return Err("bridge-disconnected".into());
        }
        match rx.await {
            Ok(r) => r,
            Err(_) => Err("bridge-disconnected".into()),
        }
    }
}

#[derive(Default)]
pub struct AssetCache {
    by_key: HashMap<String, Arc<String>>,
    order: Vec<String>,
}

impl AssetCache {
    pub fn ordered_frames(&self) -> Vec<Arc<String>> {
        self.order
            .iter()
            .filter_map(|k| self.by_key.get(k).cloned())
            .collect()
    }

    fn insert(&mut self, key: String, frame: Arc<String>) {
        if self.by_key.insert(key.clone(), frame).is_none() {
            self.order.push(key);
        }
    }
}

pub fn channel() -> (BridgeHandle, mpsc::Receiver<BridgeCommand>) {
    let (cmd_tx, cmd_rx) = mpsc::channel(32);
    (BridgeHandle { cmd_tx }, cmd_rx)
}

pub async fn run(
    addr: SocketAddr,
    tx: broadcast::Sender<Arc<String>>,
    last: Arc<RwLock<Option<Arc<String>>>>,
    scene: Arc<RwLock<SceneCache>>,
    assets: Arc<RwLock<AssetCache>>,
    mut cmd_rx: mpsc::Receiver<BridgeCommand>,
) {
    let mut seq_counter: u64 = 0;
    loop {
        let mut inflight: HashMap<u64, oneshot::Sender<Result<(), String>>> = HashMap::new();

        match TcpStream::connect(addr).await {
            Ok(stream) => {
                eprintln!("bridge: connected to {addr}");
                let (rd, mut wr) = stream.into_split();
                let mut reader = BufReader::new(rd);
                let mut line = String::new();

                'session: loop {
                    line.clear();
                    tokio::select! {
                        biased;
                        cmd = cmd_rx.recv() => {
                            match cmd {
                                Some(cmd) => {
                                    seq_counter = seq_counter.wrapping_add(1);
                                    let seq = seq_counter;
                                    let frame = encode_action(seq, cmd.module_id, &cmd.action);
                                    if wr.write_all(frame.as_bytes()).await.is_err() {
                                        let _ = cmd.reply.send(Err("bridge-disconnected".into()));
                                        break 'session;
                                    }
                                    inflight.insert(seq, cmd.reply);
                                }
                                None => return,
                            }
                        }
                        n = reader.read_line(&mut line) => {
                            match n {
                                Ok(0) => {
                                    eprintln!("bridge: closed by peer");
                                    break 'session;
                                }
                                Ok(_) => {
                                    let trimmed = line.trim_end();
                                    if trimmed.is_empty() { continue; }
                                    handle_inbound(
                                        trimmed,
                                        &tx,
                                        &last,
                                        &scene,
                                        &assets,
                                        &mut inflight,
                                    ).await;
                                }
                                Err(e) => {
                                    eprintln!("bridge: read error: {e}");
                                    break 'session;
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("bridge: connect failed: {e}");
            }
        }

        for (_, reply) in inflight.drain() {
            let _ = reply.send(Err("bridge-disconnected".into()));
        }
        sleep(Duration::from_secs(2)).await;
    }
}

fn encode_action(seq: u64, module_id: i64, action: &str) -> String {
    let action_json = serde_json::Value::String(action.into()).to_string();
    format!(
        "{{\"op\":\"module-action\",\"seq\":{seq},\"moduleId\":{module_id},\"action\":{action_json}}}\n"
    )
}

async fn handle_inbound(
    raw: &str,
    tx: &broadcast::Sender<Arc<String>>,
    last: &Arc<RwLock<Option<Arc<String>>>>,
    scene: &Arc<RwLock<SceneCache>>,
    assets: &Arc<RwLock<AssetCache>>,
    inflight: &mut HashMap<u64, oneshot::Sender<Result<(), String>>>,
) {
    let mut v: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bridge: bad JSON ({e}): {raw}");
            return;
        }
    };

    let had_op = v.get("op").is_some();
    if !had_op {
        if let Some(obj) = v.as_object_mut() {
            obj.insert("op".into(), serde_json::Value::String("snapshot".into()));
            obj.insert("v".into(), serde_json::Value::from(1));
        }
    }
    let op = v.get("op").and_then(|x| x.as_str()).unwrap_or("").to_string();

    match op.as_str() {
        "snapshot" => {
            *scene.write().await = SceneCache::from_envelope(&v);
            let arc = Arc::new(v.to_string());
            *last.write().await = Some(arc.clone());
            let _ = tx.send(arc);
        }
        "module-asset" => {
            let plugin_slug = v.get("pluginSlug").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let model_slug = v.get("modelSlug").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if plugin_slug.is_empty() || model_slug.is_empty() {
                eprintln!("bridge: module-asset missing slugs: {raw}");
                return;
            }
            let key = format!("{plugin_slug}/{model_slug}");
            let arc = Arc::new(v.to_string());
            assets.write().await.insert(key, arc.clone());
            let _ = tx.send(arc);
        }
        "action-ack" => {
            let Some(seq) = v.get("seq").and_then(|x| x.as_u64()) else { return };
            let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
            let reason = v
                .get("reason")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if let Some(reply) = inflight.remove(&seq) {
                let r = if ok {
                    Ok(())
                } else if reason.is_empty() {
                    Err("action-failed".into())
                } else {
                    Err(reason)
                };
                let _ = reply.send(r);
            }
        }
        other => {
            eprintln!("bridge: unknown op {other:?}");
        }
    }
}
