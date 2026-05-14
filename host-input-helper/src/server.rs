use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

use crate::bridge::{AssetCache, BridgeHandle};
use crate::injector::Injector;
use crate::scene::SceneCache;

const ACTION_TIMEOUT: Duration = Duration::from_secs(2);

pub async fn client_session(
    stream: TcpStream,
    peer: SocketAddr,
    mut snapshots: broadcast::Receiver<Arc<String>>,
    injector: Arc<dyn Injector>,
    last: Arc<RwLock<Option<Arc<String>>>>,
    scene: Arc<RwLock<SceneCache>>,
    assets: Arc<RwLock<AssetCache>>,
    bridge: Arc<BridgeHandle>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws = accept_async(stream).await?;
    eprintln!("client {peer}: handshake ok");
    let (mut writer, mut reader) = ws.split();

    let cached_assets = assets.read().await.ordered_frames();
    for frame in cached_assets {
        writer.send(Message::Text((*frame).clone())).await?;
    }

    if let Some(snap) = last.read().await.clone() {
        writer.send(Message::Text((*snap).clone())).await?;
    }

    loop {
        tokio::select! {
            biased;
            recv = snapshots.recv() => {
                match recv {
                    Ok(snap) => {
                        if writer.send(Message::Text((*snap).clone())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("client {peer}: lagged {n} snapshots");
                    }
                }
            }
            msg = reader.next() => {
                match msg {
                    None => break,
                    Some(Err(_)) => break,
                    Some(Ok(Message::Text(t))) => {
                        let snapshot = scene.read().await.clone();
                        let reply = handle_text(&t, &*injector, &snapshot, bridge.as_ref()).await;
                        if let Some(reply) = reply {
                            if writer.send(Message::Text(reply)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => {}
                }
            }
        }
    }
    eprintln!("client {peer}: disconnected");
    Ok(())
}

async fn handle_text(
    text: &str,
    injector: &dyn Injector,
    scene: &SceneCache,
    bridge: &BridgeHandle,
) -> Option<String> {
    let v: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return Some(err_reply(None, &format!("parse: {e}"))),
    };
    let op = v.get("op").and_then(|x| x.as_str()).unwrap_or("");
    let seq = v.get("seq").and_then(|x| x.as_u64());
    match op {
        "hello" => None,
        "intent" => {
            let kind = v.get("kind").and_then(|x| x.as_str()).unwrap_or("");
            if kind == "module-action" {
                handle_module_action(&v, seq, bridge).await
            } else {
                match injector.handle(kind, &v, scene) {
                    Ok(()) => Some(ack_reply(seq)),
                    Err(reason) => Some(err_reply(seq, &reason)),
                }
            }
        }
        _ => Some(err_reply(seq, "unknown op")),
    }
}

async fn handle_module_action(
    v: &serde_json::Value,
    seq: Option<u64>,
    bridge: &BridgeHandle,
) -> Option<String> {
    let action = v.get("action").and_then(|x| x.as_str()).unwrap_or("");
    if !is_known_action(action) {
        return Some(err_reply(seq, &format!("unknown action: {action}")));
    }
    let Some(module_id) = v.get("moduleId").and_then(|x| x.as_i64()) else {
        return Some(err_reply(seq, "missing moduleId"));
    };

    let fut = bridge.send_action(module_id, action.to_string());
    match tokio::time::timeout(ACTION_TIMEOUT, fut).await {
        Ok(Ok(())) => Some(ack_reply(seq)),
        Ok(Err(reason)) => Some(err_reply(seq, &reason)),
        Err(_) => Some(err_reply(seq, "action-timeout")),
    }
}

fn is_known_action(action: &str) -> bool {
    matches!(
        action,
        "bypass" | "disconnect" | "reset" | "randomize" | "clone" | "delete"
    )
}

fn ack_reply(seq: Option<u64>) -> String {
    match seq {
        Some(s) => format!(r#"{{"op":"ack","seq":{s}}}"#),
        None => r#"{"op":"ack"}"#.into(),
    }
}

fn err_reply(seq: Option<u64>, reason: &str) -> String {
    let r = serde_json::Value::String(reason.into()).to_string();
    match seq {
        Some(s) => format!(r#"{{"op":"err","seq":{s},"reason":{r}}}"#),
        None => format!(r#"{{"op":"err","reason":{r}}}"#),
    }
}
