use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use host_input_helper::bridge::{self, AssetCache, BridgeHandle};
use host_input_helper::injector::{Injector, LogInjector};
use host_input_helper::scene::SceneCache;
use host_input_helper::server;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

const BRIDGE_SNAPSHOT: &str = r#"{
  "t": 0.0,
  "window": {"w": 1920, "h": 1080},
  "view": {"zoom": 1.0},
  "hovered": {"kind": "none"},
  "modules": [
    {"id": 1, "name": "VCO", "pluginSlug": "Fundamental", "modelSlug": "VCO",
     "bypassed": false,
     "screenBox": [220, 180, 150, 380],
     "params":  [{"id": 0, "name": "Freq", "x": 295, "y": 240}],
     "inputs":  [{"id": 0, "name": "V/OCT", "x": 250, "y": 520}],
     "outputs": [{"id": 0, "name": "SIN",   "x": 340, "y": 520}]},
    {"id": 2, "name": "VCF", "pluginSlug": "Fundamental", "modelSlug": "VCF",
     "bypassed": false,
     "screenBox": [400, 180, 150, 380],
     "params":  [{"id": 0, "name": "Cutoff", "x": 475, "y": 240}],
     "inputs":  [{"id": 0, "name": "IN",     "x": 430, "y": 520}],
     "outputs": [{"id": 0, "name": "OUT",    "x": 520, "y": 520}]}
  ]
}"#;

// Fake bridge that just streams snapshots; ignores anything sent to it.
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

// Fake bridge that emits one module-asset, streams snapshots, and replies
// to module-action commands with action-ack frames. `fail_actions=true`
// makes it reply with ok=false so callers can test the error path.
async fn spawn_smart_fake_bridge(fail_actions: bool) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            let (rd, mut wr) = stream.into_split();

            let asset = r#"{"op":"module-asset","v":1,"pluginSlug":"Fundamental","modelSlug":"VCO","format":"svg","data":"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'><rect width='10' height='10' fill='red'/></svg>"}"#;
            if wr.write_all(format!("{asset}\n").as_bytes()).await.is_err() {
                continue;
            }

            let snap_line = format!("{}\n", BRIDGE_SNAPSHOT.replace('\n', " "));
            let mut reader = BufReader::new(rd);
            let mut line = String::new();
            let mut tick = tokio::time::interval(Duration::from_millis(50));
            tick.tick().await; // consume the immediate first tick

            loop {
                tokio::select! {
                    n = reader.read_line(&mut line) => {
                        match n {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {
                                let v: serde_json::Value =
                                    match serde_json::from_str(line.trim()) {
                                        Ok(v) => v,
                                        Err(_) => { line.clear(); continue; }
                                    };
                                line.clear();
                                if v.get("op").and_then(|x| x.as_str()) == Some("module-action") {
                                    let seq = v.get("seq").and_then(|x| x.as_u64()).unwrap_or(0);
                                    let ack = if fail_actions {
                                        format!("{{\"op\":\"action-ack\",\"seq\":{seq},\"ok\":false,\"reason\":\"simulated-failure\"}}\n")
                                    } else {
                                        format!("{{\"op\":\"action-ack\",\"seq\":{seq},\"ok\":true}}\n")
                                    };
                                    if wr.write_all(ack.as_bytes()).await.is_err() { break; }
                                }
                            }
                        }
                    }
                    _ = tick.tick() => {
                        if wr.write_all(snap_line.as_bytes()).await.is_err() { break; }
                    }
                }
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
    let assets = Arc::new(RwLock::new(AssetCache::default()));
    let injector: Arc<dyn Injector> = Arc::new(LogInjector);
    let (bridge_handle, cmd_rx) = bridge::channel();
    let bridge_handle = Arc::new(bridge_handle);

    tokio::spawn(bridge::run(
        bridge_addr,
        snap_tx.clone(),
        last.clone(),
        scene.clone(),
        assets.clone(),
        cmd_rx,
    ));

    let snap_tx_outer = snap_tx.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, peer)) = ws_listener.accept().await else { continue };
            let rx = snap_tx_outer.subscribe();
            let inj = injector.clone();
            let last = last.clone();
            let scene = scene.clone();
            let assets = assets.clone();
            let bridge_h: Arc<BridgeHandle> = bridge_handle.clone();
            tokio::spawn(async move {
                let _ = server::client_session(
                    stream, peer, rx, inj, last, scene, assets, bridge_h,
                ).await;
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
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let res = timeout(remaining, ws.next()).await;
        let Ok(Some(Ok(Message::Text(t)))) = res else {
            panic!("timeout waiting for reply seq={expected_seq}");
        };
        let v: Value = serde_json::from_str(&t).expect("valid JSON");
        let op = v.get("op").and_then(|x| x.as_str());
        if op == Some("snapshot") || op == Some("module-asset") {
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

async fn next_module_asset(ws: &mut Ws) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let res = timeout(remaining, ws.next()).await;
        let Ok(Some(Ok(Message::Text(t)))) = res else {
            panic!("timeout waiting for module-asset");
        };
        let v: Value = serde_json::from_str(&t).expect("valid JSON");
        if v.get("op").and_then(|x| x.as_str()) == Some("module-asset") {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn module_asset_replays_to_new_clients() {
    let bridge_addr = spawn_smart_fake_bridge(false).await;
    let ws_addr = spawn_helper(bridge_addr).await;

    // First client triggers the bridge -> helper asset cache fill.
    let mut ws1 = connect_ws(ws_addr).await;
    ws1.send(Message::Text(r#"{"op":"hello","v":1,"client":"a"}"#.into()))
        .await
        .unwrap();
    let asset = next_module_asset(&mut ws1).await;
    assert_eq!(asset["pluginSlug"], "Fundamental");
    assert_eq!(asset["modelSlug"], "VCO");
    assert_eq!(asset["format"], "svg");
    assert!(asset["data"].as_str().unwrap().contains("<svg"));

    // Give the helper a moment to settle.
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Second client should receive the cached asset before any snapshot.
    let mut ws2 = connect_ws(ws_addr).await;
    ws2.send(Message::Text(r#"{"op":"hello","v":1,"client":"b"}"#.into()))
        .await
        .unwrap();
    let asset2 = next_module_asset(&mut ws2).await;
    assert_eq!(asset2["pluginSlug"], "Fundamental");
    assert_eq!(asset2["modelSlug"], "VCO");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn module_action_round_trips_to_bridge() {
    let bridge_addr = spawn_smart_fake_bridge(false).await;
    let ws_addr = spawn_helper(bridge_addr).await;
    let mut ws = connect_ws(ws_addr).await;

    ws.send(Message::Text(r#"{"op":"hello","v":1,"client":"t"}"#.into()))
        .await
        .unwrap();
    let _ = next_module_asset(&mut ws).await;
    let _ = next_snapshot(&mut ws).await;

    for (i, action) in ["bypass", "disconnect", "reset", "randomize", "clone", "delete"]
        .iter()
        .enumerate()
    {
        let seq = (100 + i) as u64;
        let intent = json!({
            "op": "intent",
            "kind": "module-action",
            "action": action,
            "moduleId": 1,
            "seq": seq,
        });
        ws.send(Message::Text(intent.to_string())).await.unwrap();
        let reply = next_reply_for(&mut ws, seq).await;
        assert_eq!(reply["op"], "ack", "action {action} reply: {reply}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn module_action_unknown_action_is_err() {
    let bridge_addr = spawn_smart_fake_bridge(false).await;
    let ws_addr = spawn_helper(bridge_addr).await;
    let mut ws = connect_ws(ws_addr).await;

    let intent = json!({
        "op": "intent",
        "kind": "module-action",
        "action": "self-destruct",
        "moduleId": 1,
        "seq": 9,
    });
    ws.send(Message::Text(intent.to_string())).await.unwrap();
    let reply = next_reply_for(&mut ws, 9).await;
    assert_eq!(reply["op"], "err");
    assert!(reply
        .get("reason")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .contains("unknown action"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn module_action_propagates_plugin_error() {
    let bridge_addr = spawn_smart_fake_bridge(true).await;
    let ws_addr = spawn_helper(bridge_addr).await;
    let mut ws = connect_ws(ws_addr).await;

    let intent = json!({
        "op": "intent",
        "kind": "module-action",
        "action": "bypass",
        "moduleId": 1,
        "seq": 11,
    });
    ws.send(Message::Text(intent.to_string())).await.unwrap();
    let reply = next_reply_for(&mut ws, 11).await;
    assert_eq!(reply["op"], "err");
    assert_eq!(
        reply.get("reason").and_then(|x| x.as_str()),
        Some("simulated-failure")
    );
}
