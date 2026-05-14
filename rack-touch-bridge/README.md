# RackTouchBridge

A VCV Rack 2 plugin that broadcasts the live scene graph (module rectangles,
parameter and port centers, panel zoom/pan, hovered widget) over a localhost
TCP socket as newline-delimited JSON.

It is the metadata feeder for a Rack-aware remote-control client (Android over
RustDesk in this project's case). The client can map a touch event to the
correct desktop pixel by reading this feed instead of doing pixel-level
guesswork on the video stream.

## Why it lives in this repo

This is a VCV Rack plugin, not RustDesk code. It only sits in the
`rack-touch-bridge/` subtree because the work for the touch-control feature
is being developed on the `claude/vcv-rack-rustdesk-touch-tv6P7` branch of
this fork. It builds independently against the Rack 2 SDK and has no
dependency on the surrounding RustDesk tree.

## Build

Requirements: Rack 2 SDK (https://vcvrack.com/manual/Building#Building-plugins)
and a working C++ toolchain (MSYS2 mingw-w64 / GCC / clang).

```sh
cd rack-touch-bridge
export RACK_DIR=/path/to/Rack-SDK
make
make install     # copies the built plugin into Rack's plugin folder
```

Open Rack, add the `RackTouch / Bridge` module to a patch. The plugin starts
a TCP listener on `127.0.0.1:54321` as soon as the module is instantiated.

## Verify

```sh
nc 127.0.0.1 54321
```

You should see one JSON document per line, ~10 Hz, of the shape:

```json
{
  "t": 12345.678,
  "window": { "w": 1920, "h": 1080 },
  "view": { "zoom": 1.0 },
  "hovered": { "kind": "param", "moduleId": 7, "paramId": 2 },
  "modules": [
    {
      "id": 7,
      "name": "VCO",
      "screenBox": [400.0, 200.0, 150.0, 380.0],
      "params":  [{ "id": 0, "name": "Freq", "x": 455.0, "y": 260.0 }],
      "inputs":  [{ "id": 0, "name": "V/OCT", "x": 430.0, "y": 500.0 }],
      "outputs": [{ "id": 0, "name": "SIN",   "x": 500.0, "y": 500.0 }]
    }
  ]
}
```

Coordinates are in the Rack window's client-area pixel space, so a remote
client receiving the same screen via RustDesk can use them directly for
synthetic mouse input.

## Falsifier

The Tier-2 plan in this project depends on the screen coordinates produced
here matching the actual pixel positions seen by a remote desktop client. If
this assumption fails (custom module widgets that override transform,
unusual scaling), this prototype is where it will show up first.
