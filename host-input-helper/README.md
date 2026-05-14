# host-input-helper

Subscribes to the `RackTouchBridge` plugin on `127.0.0.1:54321`, re-broadcasts
snapshots to WebSocket clients on `127.0.0.1:54323/`, and accepts intent
messages from clients per `protocol/SPEC.md`.

This crate is **excluded from the RustDesk workspace** — build it standalone:

```sh
cd host-input-helper
cargo run --release
```

## Architecture

```
                ┌────────────────────────────────────────┐
                │  host-input-helper (this crate)        │
                │                                        │
RackTouchBridge │  ┌───────────┐    ┌──────────────────┐ │   ws://…:54323
TCP :54321 ────►│─►│ bridge.rs │───►│ server.rs (WS)   │◄┤◄─────── client(s)
                │  │ subscriber│    │ broadcasts snaps │ │
                │  └───────────┘    │ handles intents  │ │
                │                   └────────┬─────────┘ │
                │                            ▼           │
                │                    ┌───────────────┐   │
                │                    │ injector.rs   │   │
                │                    │  trait + impl │   │
                │                    └───────┬───────┘   │
                └────────────────────────────┼───────────┘
                                             ▼
                                     synthesized input
                                     (stub today; enigo
                                      on Windows next)
```

## Current behaviour

The injector is a **log-only stub**: every intent is written to stderr and
ack'd. This lets the metadata + intent pipeline be end-to-end testable
without a Windows host or real input synthesis. Plugging in `enigo` (or
`SendInput` directly) is a contained change inside `injector.rs` — none of
the other modules need to know.

## Configuration

Environment variables (all optional):

| Var                  | Default            | Purpose                              |
| -------------------- | ------------------ | ------------------------------------ |
| `BRIDGE_ADDR`        | `127.0.0.1:54321`  | RackTouchBridge TCP endpoint         |
| `WS_BIND`            | `127.0.0.1:54323`  | WebSocket bind address               |

Bind to `0.0.0.0:54323` if you want the Android device on your LAN to reach
the helper directly. Only do that on trusted networks.
