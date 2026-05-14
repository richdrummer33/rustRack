use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

use crate::injector::Injector;

pub async fn client_session(
    stream: TcpStream,
    peer: SocketAddr,
    mut snapshots: broadcast::Receiver<Arc<String>>,
    injector: Arc<dyn Injector>,
    last: Arc<RwLock<Option<Arc<String>>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws = accept_async(stream).await?;
    eprintln!("client {peer}: handshake ok");
    let (mut writer, mut reader) = ws.split();

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
                        if let Some(reply) = handle_text(&t, &*injector) {
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

fn handle_text(text: &str, injector: &dyn Injector) -> Option<String> {
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
            match injector.handle(kind, &v) {
                Ok(()) => Some(ack_reply(seq)),
                Err(reason) => Some(err_reply(seq, &reason)),
            }
        }
        _ => Some(err_reply(seq, "unknown op")),
    }
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
