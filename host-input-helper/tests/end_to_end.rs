use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use host_input_helper::injector::{Injector, LogInjector};
use host_input_helper::scene::SceneCache;
use host_input_helper::{bridge, server};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

const BRIDGE_SNAPSHOT: &str = r#"{
  "t": 0.0,
  "window": {"w": 1920, "h": 1080},
  "view": {"zoom": 1.0},
  "hovered": {"kind": "none"},
  "modules": [
    {"id": 1, "name": "VCO",
     "screenBox": [220, 180, 150, 380],
     "params":  [{"id": 0, "name": "Freq", "x": 295, "y": 240}],
     "inputs":  [{"id": 0, "name": "V/OCT", "x": 250, "y": 520}],
     "outputs": [{"id": 0, "name": "SIN",   "x": 340, "y": 520}]},
    {"id": 2, "name": "VCF",
     "screenBox": [400, 180, 150, 380],
     "params":  [{"id": 0, "name": "Cutoff", "x": 475, "y": 240}],
     "inputs":  [{"id": 0, "name": "IN",     "x": 430, "y": 520}],
     "outputs": [{"id": 0, "name": "OUT",    "x": 520, "y": 520}]}
  ]
}"#;

async fn spawn_fake_bridge() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { continue };
            let line = format!("{}\n", BRIDGE_SNAPSHOT.replace('\n', " "));
            loop {
                if stream.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    });
    addr
}

async fn spawn_helper(bridge_addr: SocketAddr) -> SocketAddr {
    let ws_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_addr = ws_listener.local_addr().unwrap();

    let (snap_tx, _) = broadcast::channel::<Arc<String>>(8);
    let last = Arc::new(RwLock::new(None));
    let scene = Arc::new(RwLock::new(SceneCache::default()));
    let injector: Arc<dyn Injector> = Arc::new(LogInjector);

    tokio::spawn(bridge::run(
        bridge_addr,
        snap_tx.clone(),
        last.clone(),
        scene.clone(),
    ));

    let snap_tx_outer = snap_tx.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, peer)) = ws_listener.accept().await else { continue };
            let rx = snap_tx_outer.subscribe();
            let inj = injector.clone();
            let last = last.clone();
            let scene = scene.clone();
            tokio::spawn(async move {
                let _ = server::client_session(stream, peer, rx, inj, last, scene).await;
            });
        }
    });

    ws_addr
}

async fn connect_ws(ws_addr: SocketAddr) -> Ws {
    let url = format!("ws://{ws_addr}/");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        match tokio_tungstenite::connect_async(&url).await {
            Ok((ws, _)) => return ws,
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => panic!("WS connect to {url} failed: {e}"),
        }
    }
}

async fn next_reply_for(ws: &mut Ws, expected_seq: u64) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let res = timeout(remaining, ws.next()).await;
        let Ok(Some(Ok(Message::Text(t)))) = res else {
            panic!("timeout waiting for reply seq={expected_seq}");
        };
        let v: Value = serde_json::from_str(&t).expect("valid JSON");
        if v.get("op").and_then(|x| x.as_str()) == Some("snapshot") {
            continue;
        }
        if v.get("seq").and_then(|x| x.as_u64()) == Some(expected_seq) {
            return v;
        }
    }
}

async fn next_snapshot(ws: &mut Ws) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let res = timeout(remaining, ws.next()).await;
        let Ok(Some(Ok(Message::Text(t)))) = res else {
            panic!("timeout waiting for snapshot");
        };
        let v: Value = serde_json::from_str(&t).expect("valid JSON");
        if v.get("op").and_then(|x| x.as_str()) == Some("snapshot") {
            return v;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn end_to_end_pipeline() {
    let bridge_addr = spawn_fake_bridge().await;
    let ws_addr = spawn_helper(bridge_addr).await;
    let mut ws = connect_ws(ws_addr).await;

    ws.send(Message::Text(
        r#"{"op":"hello","v":1,"client":"test"}"#.into(),
    ))
    .await
    .unwrap();

    let snap = next_snapshot(&mut ws).await;
    assert_eq!(snap["op"], "snapshot");
    assert_eq!(snap["v"], 1);
    assert!(snap["modules"].is_array());
    assert_eq!(snap["modules"][0]["id"], 1);

    let intents = vec![
        json!({"op":"intent","kind":"tap",
               "target":{"kind":"param","moduleId":1,"paramId":0},"seq":1}),
        json!({"op":"intent","kind":"knob-drag",
               "moduleId":1,"paramId":0,"deltaPx":15.5,"seq":2}),
        json!({"op":"intent","kind":"cable",
               "from":{"moduleId":1,"port":"output","portId":0},
               "to":{"moduleId":2,"port":"input","portId":0},"seq":3}),
        json!({"op":"intent","kind":"pan","dx":40.0,"dy":-20.0,"seq":4}),
        json!({"op":"intent","kind":"zoom","factor":1.1,
               "anchor":{"x":960,"y":540},"seq":5}),
        json!({"op":"intent","kind":"context",
               "target":{"kind":"module","moduleId":1},"seq":6}),
    ];

    for intent in intents {
        let seq = intent["seq"].as_u64().unwrap();
        ws.send(Message::Text(intent.to_string())).await.unwrap();
        let reply = next_reply_for(&mut ws, seq).await;
        assert_eq!(reply["op"], "ack", "intent seq={seq}: {reply}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_intent_kind_is_err() {
    let bridge_addr = spawn_fake_bridge().await;
    let ws_addr = spawn_helper(bridge_addr).await;
    let mut ws = connect_ws(ws_addr).await;

    let bogus = json!({"op":"intent","kind":"do-the-thing","seq":42});
    ws.send(Message::Text(bogus.to_string())).await.unwrap();
    let reply = next_reply_for(&mut ws, 42).await;
    assert_eq!(reply["op"], "err");
    assert!(reply
        .get("reason")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .contains("unknown intent kind"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_op_is_err() {
    let bridge_addr = spawn_fake_bridge().await;
    let ws_addr = spawn_helper(bridge_addr).await;
    let mut ws = connect_ws(ws_addr).await;

    let bogus = json!({"op":"yolo","seq":7});
    ws.send(Message::Text(bogus.to_string())).await.unwrap();
    let reply = next_reply_for(&mut ws, 7).await;
    assert_eq!(reply["op"], "err");
}
