# Parking lot

Ideas weighed and not built, with what stands in the way.

- **Editing in the source column.** It is read-only and colored by a display
  scanner; editing there waits for iced's `text_editor` to take
  highlighting. Dropped `$EDITOR` plans: users have their own editors, and
  "Go to your editor" focuses theirs instead.
- **A shared rule library.** The library is an in-repo list
  (`niri-center/src/library.rs`) of rules from niri's default config, wiki
  and issues, shared niri configs, and our own, each naming its source
  (researched 2026-10-10). An app lists every app ID it goes by across
  packagings, and each rule carries example windows: tests check that it
  reaches them under each ID and that niri accepts it. A
  community-maintained source would need a fetch (network access the
  sandbox does not have) and a review story.
- **Find by pressing keys** sees only what niri passes on: combos with Mod
  never reach the app, so it finds the key, not the combo.
- **Ecosystem plays**: shell integration recipes (Noctalia,
  DankMaterialShell snippets as reviewable diffs), display profiles per set
  of connected outputs, a Nix mode writing an optional-include overrides
  file.
- **i18n and packaging** (AUR, COPR, flatpak).
