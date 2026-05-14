# Touch-Control Wire Protocol

Version: `1` (unstable; bump on breaking changes).

This is the contract between three independent processes:

| Process              | Role                                            |
| -------------------- | ----------------------------------------------- |
| `RackTouchBridge`    | VCV Rack 2 plugin; emits scene metadata.        |
| `host-input-helper`  | Subscribes to bridge; serves clients; injects.  |
| Client (web/Android) | Renders touch zones; emits semantic intents.    |

Each of the three is developed against this contract alone. Changing
implementations must not break the JSON shape unless `version` is bumped
and both sides handshake on the new version.

## Transports

| Endpoint                  | Direction       | Transport                          | Producer            |
| ------------------------- | --------------- | ---------------------------------- | ------------------- |
| `tcp://127.0.0.1:54321`   | server → client | newline-delimited JSON (read-only) | `RackTouchBridge`   |
| `ws://127.0.0.1:54323/`   | bidirectional   | text frames, one JSON per frame    | `host-input-helper` |

Clients talk **only** to the host helper (`54323`). The bridge port
(`54321`) is internal: the helper subscribes to it. The bridge is local-only
(`INADDR_LOOPBACK`); the helper may bind a LAN interface later.

## Snapshot (server → client)

Identical to the bridge's snapshot, re-wrapped in an envelope:

```json
{
  "op": "snapshot",
  "v": 1,
  "t": 12345.678,
  "window": { "w": 1920, "h": 1080 },
  "view":   { "zoom": 1.0 },
  "hovered": { "kind": "param", "moduleId": 7, "paramId": 2 },
  "modules": [
    {
      "id": 7,
      "name": "VCO",
      "screenBox": [400.0, 200.0, 150.0, 380.0],
      "params":  [{ "id": 0, "name": "Freq",  "x": 455.0, "y": 260.0 }],
      "inputs":  [{ "id": 0, "name": "V/OCT", "x": 430.0, "y": 500.0 }],
      "outputs": [{ "id": 0, "name": "SIN",   "x": 500.0, "y": 500.0 }]
    }
  ]
}
```

All coordinates are in Rack-window client-area pixels — the same pixel
space a RustDesk video stream of that window will deliver.

Cadence: ~10 Hz, or whenever the bridge has a new sequence number.

## Hello (client → server, optional)

```json
{ "op": "hello", "v": 1, "client": "web-prototype" }
```

If absent, the helper assumes `v: 1`. If `v` does not match, the helper
closes the socket with an `err` frame.

## Intents (client → server)

Every intent carries a client-monotonic `seq` so the helper can `ack` or
`err` it. The helper translates intents into synthesized mouse/keyboard
events on the host. Intents are **semantic**, not raw pointer streams:
the client is responsible for gesture recognition.

### `tap`

```json
{ "op": "intent", "kind": "tap",
  "target": { "kind": "param", "moduleId": 7, "paramId": 2 },
  "seq": 17 }
```

Single click at the target's center. `target.kind` ∈
`param | input | output | module | point`. For `point`, supply `x` and `y`
in window pixels instead of ids.

### `knob-drag`

```json
{ "op": "intent", "kind": "knob-drag",
  "moduleId": 7, "paramId": 2,
  "deltaPx": 40.0,
  "fine": false,
  "seq": 18 }
```

Synthesized mouse-down at the param center, vertical drag by `deltaPx`
pixels (positive = increase), mouse-up. `fine` requests Rack's
fine-control modifier (Ctrl on Win, ⌘ on macOS).

### `cable`

```json
{ "op": "intent", "kind": "cable",
  "from": { "moduleId": 7,  "port": "output", "portId": 0 },
  "to":   { "moduleId": 12, "port": "input",  "portId": 1 },
  "seq": 19 }
```

Synthesized mouse-down at `from` jack center, drag to `to` jack center,
mouse-up. Helper validates both endpoints exist in the most recent
snapshot before injecting.

### `pan`

```json
{ "op": "intent", "kind": "pan", "dx": 30.0, "dy": -10.0, "seq": 20 }
```

Pan the rack view by `(dx, dy)` window-pixels. Implementation: middle-drag
on rack background, or scroll-wheel + Shift, depending on platform.

### `zoom`

```json
{ "op": "intent", "kind": "zoom", "factor": 1.1,
  "anchor": { "x": 960, "y": 540 }, "seq": 21 }
```

Multiply zoom by `factor` centered on `anchor` (window-pixel). Helper
injects Ctrl+scroll at the anchor position.

### `context`

```json
{ "op": "intent", "kind": "context",
  "target": { "kind": "module", "moduleId": 7 },
  "seq": 22 }
```

Right-click on the target. Reuses `target` shape from `tap`.

### `set-param` (optional, later)

```json
{ "op": "intent", "kind": "set-param",
  "moduleId": 7, "paramId": 2, "value": 0.65, "seq": 23 }
```

Direct value write. Requires the helper to drive Rack's engine through a
back-channel (out of scope for v1; intents above synthesize input only).

## Acks (server → client)

```json
{ "op": "ack", "seq": 18 }
{ "op": "err", "seq": 19, "reason": "unknown-port" }
```

`reason` is informational; clients should treat any non-`ack` as a soft
failure and surface it to the user.

## Versioning rules

* Adding a new optional field to a message: **not** a breaking change.
* Removing a field, renaming a field, changing a field's type, adding a
  new required field, or adding a new `op`/`kind` that older helpers
  must understand: **breaking**. Bump `v`.
* Clients send their supported `v` in `hello`. Helpers accept ≤ their own
  `v` and refuse anything higher.

## Out of scope

* Patch save/load.
* Module add/remove (drag-from-browser).
* Cable disconnect / re-route.
* Audio/MIDI routing.

These will get their own `kind`s in a later version once v1 is exercised.
