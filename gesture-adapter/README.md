# Gesture Adapter (web client)

A static web client that consumes touch-control snapshots and emits semantic
intents per `protocol/SPEC.md`. Single page, no build step, vanilla JS.

This is the **prototype** form of the client. The same gesture-recognition
code can later be lifted into a Flutter/Android app that also displays the
RustDesk video stream. The decoupling is intentional: this app validates
the metadata + intent pipeline against `host-input-helper` without any
RustDesk dependency.

## Run

It's a static page, but `fetch()` of `../protocol/mock-snapshot.json` needs
HTTP, so serve the repo root:

```sh
python3 -m http.server 8000
```

Then open in a mobile or desktop browser:

* **Live mode** (default): `http://<host>:8000/gesture-adapter/`
  Connects to `ws://localhost:54323/` (the host-input-helper).
* **Mock mode**: `http://<host>:8000/gesture-adapter/?mock=1`
  Loads a static three-module scene from `protocol/mock-snapshot.json`.
  No network, no helper. Use this to iterate on gesture recognition.
* **Live with custom host**: `?host=192.168.1.10&port=54323`

## Gesture map

| Gesture                          | Intent                              |
| -------------------------------- | ----------------------------------- |
| Tap a knob/jack/module           | `tap`                               |
| Vertical drag on knob            | `knob-drag` (deltaPx = -dy)         |
| Drag from jack A to jack B       | `cable`                             |
| One-finger drag on rack bg       | `pan` (streamed)                    |
| Two-finger pinch                 | `zoom` (streamed)                   |
| Long-press on target (≥ 600 ms)  | `context`                           |

Holding two fingers down cancels the in-progress single-finger gesture and
switches to pinch mode.

## Files

* `index.html` — single-page shell, status bar and log overlay.
* `app.js`     — connection, rendering, gesture recognition, intent emit.
* `style.css`  — dark theme, color-coded hit zones.
