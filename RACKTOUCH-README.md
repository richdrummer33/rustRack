# Rack Touch on RustDesk

This project lets you control [VCV Rack 2](https://vcvrack.com/) from a phone
or tablet over a [RustDesk](https://rustdesk.com/) remote session, and have
the touches feel like they belong to Rack rather than to a generic remote
desktop. You tap a knob and it acts like a knob. You drag from one jack to
another and a cable appears. You pinch and the rack zooms. The system can
tell what you are pointing at because the Rack instance tells it, in real
time, where every module, knob and jack actually is on the screen.

If you only want to get it running, read the **Setup** section below. If you
are curious about how the pieces fit together, or you are trying to debug
something, the **How it works** and **Where to look when things break**
sections are for you.

---

## Setup

You will need three machines or three roles on one machine: the **host**
that runs VCV Rack, the **client** that has the touchscreen (a phone, a
tablet, or for now a touchscreen browser), and a working **RustDesk**
connection between them. Most people develop on a single host, with the
phone connecting in over the local network.

The steps below assume Windows on the host, since that is the platform the
input injector targets. The metadata pipeline itself is cross-platform and
the project will build on Linux and macOS, but you will only see the log
injector there, not real synthesized clicks.

1. **Install the Rack-side plugin.** Open a terminal in the
   `rack-touch-bridge/` folder, point `RACK_DIR` at your Rack 2 SDK, and run
   `make` followed by `make install`. Start VCV Rack, right-click in the
   rack and add the module called `RackTouch / Bridge`. Once that module is
   in your patch, the plugin is running. It opens a local TCP port on
   `127.0.0.1:54321` and sits there feeding out a description of your patch
   several times a second.

2. **Run the host helper.** In another terminal, change into
   `host-input-helper/` and run `cargo run --release`. This is the bridge
   between the Rack plugin and the touch client. It connects to the Rack
   plugin on port `54321`, opens a WebSocket server on
   `127.0.0.1:54323`, and waits for a client to connect. On Windows it will
   also synthesize mouse and keyboard input. On Linux and macOS it will
   only print what it would have done, which is useful for development.

3. **Open the touch client.** The current client is a small web page in
   `gesture-adapter/`. From the repo root, run `python3 -m http.server 8000`
   and then on the phone visit
   `http://<host-lan-ip>:8000/gesture-adapter/?host=<host-lan-ip>`. The page
   should connect to the helper and show a wireframe of your Rack patch.
   That wireframe is the proof that the metadata link is alive. A future
   Flutter or Android client will replace this page and overlay the same
   gesture layer on top of the RustDesk video stream.

4. **Start the RustDesk session.** Connect from the phone to the host as
   you normally would with RustDesk so that the actual Rack window is
   visible on the phone. The gesture client will sit on top of the video
   feed and translate your touches into the right desktop actions on the
   host.

If you only want to play with gesture recognition without involving Rack at
all, open the gesture adapter with `?mock=1` instead. That loads a fake
three-module scene from `protocol/mock-snapshot.json` and does not need the
helper or the plugin to be running.

---

## What this thing actually is

VCV Rack is a desktop application. RustDesk can already stream its window
to a phone and send pixel-level mouse events back. The problem is that a
finger on a phone is not a mouse. You cannot reliably right-click. You
cannot delicately drag a knob by half a pixel. You cannot tell the host,
through a pixel position alone, that a long press should mean a
context menu and a fast flick should mean a pan.

This project gives the phone enough structural information about the Rack
patch to do something smarter. Rather than guessing from pixels, the phone
knows that the rectangle at coordinates `(220, 180, 150, 380)` is a VCO
module, that the small circle inside it is parameter zero called `Freq`,
and that the jack below it is the `V/OCT` input. When you touch the screen,
the phone decides what kind of gesture you made, names the target you
were aiming at, and sends a short semantic message describing what you
meant. The host then carries that out using normal mouse and keyboard
input, which is exactly what Rack already knows how to respond to.

The result, from a user perspective, is that touch in a remote Rack
session stops feeling like fighting with a tiny mouse and starts feeling
like operating a modular synthesizer that happens to be very far away.

---

## How it works

There are four pieces, connected in a straight line.

**The Rack plugin** (`rack-touch-bridge/`) lives inside VCV Rack itself.
Once you add its module to a patch it opens a TCP socket on port `54321`
and, several times per second, sends out a newline-terminated JSON object
that describes the current scene. That description includes the size of
the Rack window, the current zoom and pan of the rack view, and one entry
per module containing the module's screen rectangle, its parameter
positions, and its input and output jack positions. This is the
ground-truth feed for everything downstream. Nothing else in the system
guesses where things are; they all read it from here.

**The host helper** (`host-input-helper/`, written in Rust) sits between
the plugin and the touch client. On startup it makes two connections. It
connects out to the plugin on `54321` to subscribe to snapshots, and it
opens its own WebSocket server on `54323` to accept touch clients. When a
snapshot arrives from the plugin, the helper does two things with it. It
parses the snapshot into an in-memory **scene cache** that maps module IDs
to coordinates, and it forwards the raw snapshot text to every connected
WebSocket client. The scene cache is what lets the helper answer questions
like, when the client says "tap parameter zero of module seven", which
pixel on the host's screen does that correspond to. The forwarded snapshot
is what lets the client draw and reason about the scene at all.

The helper also accepts **intents** coming back from the client. An intent
is a small JSON message that names a gesture and a target rather than a
pixel. It might say "tap parameter zero of module seven", or "drag a cable
from output zero of module one to input zero of module two", or simply
"pan the rack view by these deltas". Every intent carries a monotonically
increasing sequence number, and the helper replies with an ack for that
sequence number once it has handled the message. If the helper cannot
make sense of an intent, or the target no longer exists, it replies with
an error tagged with the same sequence number so the client can recover.

When the helper decides what physical action an intent should turn into,
it hands the result to an **injector**. On Windows the real injector uses
the `enigo` crate to move the mouse, click, drag and scroll, which is the
same mechanism a normal automation tool would use. On Linux and macOS the
injector is a log stub that prints what it would have done. The injector
is a trait, so swapping in a different backend later, say a native Win32
SendInput implementation or a platform-specific accessibility API, only
touches one file.

**The gesture client** (`gesture-adapter/`) is the touchscreen-facing
piece. Today it is a tiny static web page; tomorrow it will be a Flutter
or Android application that lives on top of the RustDesk video. Either
way, it is the piece that turns raw touch events into semantic intents.
It opens a WebSocket to the helper, receives the snapshot feed, and uses
that snapshot to draw a hit-test layer mirroring the host's Rack window.
When you tap, drag, pinch or long-press, the client uses a small state
machine to decide which gesture you actually performed, identifies the
target you were aiming at by looking it up in the scene, packages that
into an intent, and sends it back over the same socket. It then waits for
the ack so it can stay in sync with what the host actually did.

**The wire protocol** is documented in `protocol/SPEC.md` and is the
contract everything else respects. Snapshots and intents are both small
JSON objects. Snapshots flow only from helper to client. Intents flow
only from client to helper, and every intent is acknowledged exactly once
with either an ack or an err carrying the same sequence number. The
protocol version is currently one and is included in every snapshot, so
that an older client and a newer helper can recognize each other when
they disagree.

---

## Where to look when things break

The simplest test, when something is wrong, is to walk the pipeline from
one end to the other and find the first link that has gone quiet.

If nothing in the system seems to be working at all, start at the plugin.
Run `nc 127.0.0.1 54321` (or any other TCP client) on the host. You
should see a continuous stream of JSON lines, one per snapshot. If you
don't, the Rack plugin is not loaded, or the `RackTouch / Bridge` module
has not been added to the current patch.

If the plugin is talking but the touch client is not, look at the helper.
It prints connection events to stderr, including when it has subscribed to
the plugin and when each WebSocket client has connected. You can also
point a small WebSocket testing tool at `ws://127.0.0.1:54323/`. After
sending the literal text `{"op":"hello","v":1,"client":"test"}` you
should immediately receive a `snapshot` message back. That confirms the
helper is alive, has parsed at least one snapshot, and is willing to talk
to clients.

If the client receives snapshots but your gestures don't seem to do
anything, the question is whether the intents are being sent, received,
or executed. The helper logs every intent it gets and every ack or error
it replies with. If you see intents arriving and being ack'd but no
visible effect on the host, you are on Linux or macOS (where the
injector is the log stub) or your `INJECTOR` environment variable is set
to `log`. To force the real input synthesizer on Windows, set
`INJECTOR=enigo` before starting the helper.

The integration tests in `host-input-helper/tests/end_to_end.rs` exercise
the full snapshot-intent-ack loop in process, against the log injector,
so if they pass and your real setup does not work, the fault is almost
certainly in the plugin connection, the WebSocket transport between the
phone and the host, or the platform-specific injector — not in the
protocol or the helper's core logic. The unit tests in
`host-input-helper/src/scene.rs` cover the scene cache, which is the
piece that turns a target name into a screen pixel, so a failure there
points at a snapshot format change or a stale parser.

Default ports are `54321` for the plugin's TCP listener and `54323` for
the helper's WebSocket server. Both can be overridden via the
`BRIDGE_ADDR` and `WS_BIND` environment variables on the helper, in case
something else on your machine is already using them.

---

## Recent additions: panel artwork and module actions

Two pieces have been added on top of the original snapshot/intent pipeline
to make the touch client less reliant on the host's video stream.

The first is **panel artwork shipping**. The plugin now ships each module
type's actual panel SVG to the helper exactly once, the first time it sees
that module type in a session. The helper caches every asset it sees and
replays the cache to any client that connects later, so a phone joining
mid-session gets the artwork it needs without waiting for new modules to
appear. The gesture client uses the cached SVG to draw a faithful image of
each module's panel inside the snapshot-derived rectangle, falling back to
the wireframe label when no asset is available. Touch routing is unchanged
— it still goes through the snapshot's knob and jack coordinates, so the
SVG is purely visual.

The second is **module actions**. The protocol now carries a new intent
called `module-action` covering the six universal Rack operations:
`bypass`, `disconnect`, `reset`, `randomize`, `clone`, and `delete`. The
phone shows an action sheet on long-press of a module, and the chosen
action round-trips from phone to helper to plugin to Rack's API, going
through Rack's normal undo history just as if the user had right-clicked
the module on the host. Plugin-defined custom menu items are not yet
exposed — that needs real menu introspection and is deferred until the
asset and action paths have been exercised in practice.

Both additions rely on a new bidirectional channel on the existing TCP
port 54321. Snapshots and assets flow from plugin to helper as before;
action requests now flow from helper to plugin and return as `action-ack`
frames that the helper translates back into client-facing `ack` or `err`
messages tagged with the original client-side sequence number.

## Where the code lives

`rack-touch-bridge/` is the VCV Rack 2 plugin, written in C++ against the
Rack SDK. `host-input-helper/` is the Rust helper, exposed as both a
library and a binary so that tests and future tooling can use its
internals. `gesture-adapter/` is the current touchscreen client, a static
HTML and JavaScript page that will eventually be superseded by a Flutter
application. `protocol/` is the source of truth for the wire format,
including a JSON Schema for snapshots and intents and a mock snapshot
useful for offline development. The surrounding tree is upstream RustDesk
and is unrelated to this feature.
