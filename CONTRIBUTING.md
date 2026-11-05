# Contributing

How code is written and checked here. How claims about it are justified is
`docs/edit-epistemology.md`; why the app is shaped as it is lives in the decision records,
`docs/ADR-*.md`. They state intent and its reasons and name no file, line or
symbol of the code, so a refactor leaves them alone.

## Building and running

Requires [`just`](https://github.com/casey/just) (`cargo install just`),
[`ast-grep`](https://ast-grep.github.io) for `just check` (`cargo install
ast-grep --locked`, or the distribution's package), and for the browser
demo [`wasm-pack`](https://github.com/drager/wasm-pack) (`cargo install
wasm-pack`).

```sh
just run           # or: cargo run          (starts the app)
just check         # fmt + ast-grep rules + clippy + tests: the CI gate
just audit         # cargo audit + cargo deny
just ui-shots      # render every screen headlessly to target/ui-shots
just web           # the browser demo, into target/site
just licenses      # regenerate THIRD_PARTY_LICENSES.md
just packages      # .deb and .rpm of the release build (cargo-deb, cargo-generate-rpm)
```

The browser demo is the same code built for `wasm32-unknown-unknown`:
niri's IPC, the sandbox, `niri validate` and file writes stay out of it, and
it opens niri's default config (`niri-center/demo/`). Its entry is
`start` in `niri-center-ui/src/lib.rs`. To serve it locally:

```sh
just web
cp niri-center-ui/web/index.html target/site/
python3 -m http.server -d target/site
```

`?page=displays` (any page name from `niri-center --help`) opens on that
page. `.github/workflows/pages.yml` publishes it with GitHub Pages on every
push to main.

niri-ipc is pinned to the niri release the app targets (`=26.4.0` for niri
26.04); bump it in lockstep with niri, then refresh the action catalog (see
Reminders).

## Releasing

`.github/workflows/release.yml` builds a release in Debian 12, for x86_64
and aarch64: a tarball, a .deb and an .rpm each. A binary needs at least the
glibc it was linked against, and Debian 12's is older than that of any
distribution niri 26.04 runs on. Every package is installed and started on
Debian 12, Ubuntu 24.04, Fedora 43 and AlmaLinux 10 before the release is
drafted.

1. Set the version in `niri-center-ui/Cargo.toml`.
2. Run the Release workflow by hand: a dry run, whose packages stay as the
   run's artifacts to try.
3. Tag `v<version>` and push the tag. The workflow drafts the release with
   the packages and `SHA256SUMS`; publish it.
4. In `dist/aur/niri-center/PKGBUILD` set `pkgver` and `pkgrel=1`, run
   `updpkgsums` and `makepkg --printsrcinfo > .SRCINFO`, and push both to
   the AUR's niri-center repository. `dist/aur/niri-center-git/PKGBUILD`
   follows main by itself.

The workflow fails when the binary links a library the packages don't
name: add it to both PKGBUILDs' `depends` and to the check's list. The .deb
and .rpm find their dependencies themselves.

## Provenance and licensing

- **Independent re-implementation**, not a port or a fork. The UX re-imagines
  [NiriMod](https://github.com/srinivasr/nirimod) (MIT) and
  [niri-settings](https://github.com/stefonarch/niri-settings) (GPL-2.0); the
  code is written from scratch. Do not merge GPL-2.0-only code into this
  GPL-3.0 project (license incompatibility).
- **niri-ipc is the only GPL in the tree** and it is why the project is
  GPL-3.0-or-later. If permissive licensing is ever wanted: replace
  `niri-center/src/ipc.rs` with a clean-room JSON-over-Unix-socket client
  (niri's wiki documents the wire protocol; the surface here is ~7 request
  types and ~20 event variants).
- **LLM assistance disclosure**: substantial portions were written with model
  assistance, and layout/interaction reviews used LLM personas with
  human-written remarks. Adopted findings (contrast floor, remove-undo,
  `used-by-N` visualizer hints) are in-repo as features, not as prose.

## Code rules

- Errors are `thiserror` enums with a variant per case, never `String`s.
- No `unwrap`/`expect`/`panic!` outside tests. Clippy denies them, and
  `clippy.toml` lets tests have them.
- `pub` only where a crate boundary needs it.
- Full words in names (`index`, not `idx`), closure parameters and
  lifetimes included (`'app`, `'store`): a name says what it holds. `x` and
  `y` stay for coordinates, where they are the words.
- Parsing is the hand-written parser in `niri-center/src/kdl`; the only
  regexes are niri's own `match` patterns.
  Source coloring (`highlight.rs`) is a display-only scanner, never a way
  to read the config.
- A string that names one of a set the code knows at compile time is an
  enum (`Setting`, `OutputSetting`, `Animation`, `Curve`, `Modifier`,
  `RuleProperty`, `MatchName`, `KnownApp`, `DraftKey`); text people read and
  niri's own words written into the file stay strings. Column and window
  sizes stay text (`SizeEntry`): a number type would not keep what was
  written (`1.0`, `0.50`), and the edit compares entries as written.
- Code that needs no iced type goes in the library, where it is tested
  without a window.
- No `Arc`, `Mutex`, `RefCell`, `dyn` or statics holding app state. What
  blocks (the disk, niri) has one owner that takes commands and sends
  replies, so there are no locks and every transition is tested as a
  function.
- Keep the workspace lint set (`[workspace.lints.clippy]`): it encodes real
  review findings.
- The ast-grep rules in `lint/` (`ast-grep scan`, `ast-grep test`) refuse
  what clippy cannot see: a `Box` (hold the value, borrow it, or make the
  code generic), state shared through a lock, cell, reference count or
  static (give it one owner and send it messages), `_ = x;` (handle it or
  say why it cannot matter), and `&'static str` naming a closed set (an
  enum; text people read and niri's own words are `Words`).
- [corrode's defensive programming](https://corrode.dev/blog/defensive-programming/)
  applies: slice patterns before indexing, no catch-all arms, `TryFrom` for
  conversions that can fail, enums for flags. For actors,
  [barafael's posts](https://barafael.github.io/): one owner reached by
  messages, messages that own their data, shutdown when the channel
  closes, tests that await the loop instead of sleeping.
- Lints are fixed in the code, never silenced with a local `allow`. A lint
  that is wrong for a kind of code everywhere changes level in
  `Cargo.toml`, with the reason beside it (`large_enum_variant`: its fix is
  a Box). Tests may index, unwrap and panic (`clippy.toml`): there a panic
  is a failing test.
- A failure that cannot matter is written `if x.is_err() { // why }`, never
  `let _ =`; a value dropped on purpose is `drop(x)`.

## Verification

- `just check` (fmt, the ast-grep rules, clippy for the desktop and the wasm build, tests) and
  `just audit` (rustsec, cargo-deny). CI runs the same. `just check` lints
  the wasm build, so a new native-only call fails CI instead of the demo.
- Clippy's nursery lints are unfinished and often noisy: `just lint-nursery`
  now and then, taking what reads better, never a gate (decided
  2026-10-10).
- Keep justfile recipes plain sequential commands: no shell plumbing
  (xargs, pipes, settings overrides) and no tools beyond cargo, its
  subcommands, wasm-pack and ast-grep. CI's test image installs ast-grep
  from the same dated Arch snapshot as niri.
- Workflows: CI runs zizmor (its action, pinned, read-only, `auditor`
  persona); locally, `zizmor .github/workflows/` before changing a
  workflow. Actions are pinned to commits, jobs get only the permissions
  they use, checkouts keep no credentials.
- CI's tests run in this repo's test image (`Containerfile.ci`: niri at the
  pinned version, the toolchain, a font), which the CI workflow builds into
  the repo's container registry when its tag is new. The tag is the first
  12 hex digits of `cat Containerfile.ci rust-toolchain.toml | sha256sum`,
  kept as `CI_IMAGE_TAG` in `.github/workflows/ci.yml`; after changing
  either file, update it, or the image job fails and says the new one. To
  move to newer niri and Arch packages, move both dates in the
  Containerfile together. Tests that ask niri skip where it is not
  installed, except with `CI` set: there a missing niri fails the suite.
- UI tests are headless (`iced_test`, `niri-center-ui/src/ui_tests.rs`):
  they drive the real `view`/`update` and assert behaviour or computed
  properties (contrast). Screens rendered by `just ui-shots` are for human
  review and are never compared as truth.
- Tests point every folder the app writes at a temp dir and never touch the
  real config or state (`docs/ADR-files-and-sandbox.md`).
- The browser demo: clippy passes either renderer, so click through the
  demo after touching rendering, over WebGPU, WebGL and the software
  renderer (`docs/ADR-browser-demo.md`).

## Reminders

- `cargo run` starts the app (the workspace's one binary); `--workspace`
  covers the library's tests explicitly (CI does).
- After a niri-ipc bump, `UPDATE_ACTIONS=1 cargo test -p niri-center`
  refreshes the action catalog the browser build ships.
