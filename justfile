# niri-center — task runner.
# `just` alone starts the app; `just check` is the whole CI gate.

# Build and launch the settings GUI (workspace default member).
run:
    cargo run

# Run every test in the workspace.
test:
    cargo test --workspace

# Clippy, warnings as errors.
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Clippy's nursery lints, now and then: advice to weigh, not a gate.
lint-nursery:
    cargo clippy --workspace --all-targets -- -W clippy::nursery

# Format everything.
fmt:
    cargo fmt

# The CI gate: formatting, then clippy, then tests. Stops at the first failure.
check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy -p niri-center-ui --target wasm32-unknown-unknown -- -D warnings
    cargo test --workspace

# The browser demo, built by wasm-pack (wasm-bindgen, then wasm-opt) into
# target/site; add niri-center-ui/web/index.html to serve it.
web:
    wasm-pack build niri-center-ui --target web --profile web --no-typescript --no-pack --out-dir ../target/site --out-name niri-center

# Render every screen headlessly to target/ui-shots for a human to look at.
ui-shots:
    cargo test -p niri-center-ui render_screens_for_review -- --ignored

# Render every page from top to bottom at a desktop, half and third width
# to target/ui-pages, to look over whole panes.
ui-pages:
    cargo test -p niri-center-ui render_whole_pages_for_review -- --ignored

# Supply-chain gates: rustsec advisories, then cargo-deny.
audit:
    cargo audit
    cargo deny check

# Regenerate the third-party license report.
licenses:
    cargo about generate about.hbs > THIRD_PARTY_LICENSES.md

# Optimized single-binary build for distribution.
release:
    cargo build --release -p niri-center-ui
