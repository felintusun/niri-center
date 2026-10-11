# Browser demo fonts

A browser lends the app no fonts, so the in-browser demo
(`--target wasm32-unknown-unknown`) brings its own:

- `FiraMono-Regular.ttf` for config paths and diffs, and
  `FiraSans-SemiBold.ttf` for the headings' weight. SIL Open Font License
  1.1, see `Fira-LICENSE.txt`. The regular UI face, Fira Sans, comes with
  iced.

The demo's config and sample data are the library's: `niri-center/demo/`
and `niri-center/src/demo.rs`.
