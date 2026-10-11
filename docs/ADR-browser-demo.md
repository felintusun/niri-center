# ADR: the app also runs in a browser

**Status:** accepted 2026-10-06. Held provisionally (AGENTS.md
[provisional]).

**Decision.** The app also builds for `wasm32-unknown-unknown` and is
published with GitHub Pages (`.github/workflows/pages.yml`), linked from the
README.

- Everything that reaches niri goes through the niri actor in `niri.rs`,
  whose browser half answers with `demo.rs` data or says the desktop app
  does this, and the disk actor keeps the demo's files in memory.
- What the app may do where it runs (write files, apply the sandbox, show
  the demo's notes) is one `Platform` value on the session, so tests can be
  the demo too; only the renderer, chosen at build time, stays a `cfg!`.
- `niri-ipc` (Unix sockets) is native-only, so the action catalog ships as
  `niri-center/src/actions.txt`, which a native test keeps equal to
  niri-ipc.
- The demo draws with WebGPU, else WebGL, else tiny-skia; iced picks at
  start. WebGL is the one most niri users get: Firefox 157 blocklists
  WebGPU on Mesa (`FEATURE_FAILURE_BROKEN_DRIVER`), and Chrome 155 on Linux
  offers `navigator.gpu` with no adapter, where a WebGPU-only build
  crashed.
- No MSAA in the browser (`settings` in `niri-center-ui/src/lib.rs`): over
  WebGL it left a page's canvas drawings on the next page and lost icons
  (iced-rs/iced#2400). Diagonal icon strokes are slightly jagged there.
- `theme::lift` drops shadows in the browser, since tiny-skia cannot draw
  them and the renderer is known only at run time.
- wasm-pack builds it with the `web` profile (opt-level "s", opt-level 3
  for the software renderer's crates, wasm SIMD from `.cargo/config.toml`,
  then wasm-opt).
- `std::time::SystemTime::now` panics in the browser; the library reads the
  clock only in `clock.rs`, through `web-time` on wasm.

**Why.** Trying the app should not need niri, or trust in a binary.

**Rejected alternatives.** tiny-skia alone: 14 to 175 ms a frame at
1280×900 and up to 480 ms at twice the pixel density, against 1 to 3 ms
on the GPU. WebGPU without WebGL: the CPU for Firefox on Linux, and a crash
in Chrome on Linux. MSAA over WebGL: see above. opt-level 3 for the whole
build: 9% more download, no faster frame on the GPU. Shadows under
tiny-skia: it draws blurred shadows as slabs it never clips or repaints.

**Falsifiers.** A page that cannot be shown honestly without niri; the
browser build holding back a desktop feature.
