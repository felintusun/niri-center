# ADR: where the app writes, and the sandbox around it

**Status:** accepted (sandbox 2026-10-03, session bus 2026-10-06, XDG
placement 2026-10-11). Held provisionally (AGENTS.md [provisional]).

**Decision.**

- **Files go where XDG says.** `xdg.rs` is the one place that names the
  app's folders: preferences in `$XDG_CONFIG_HOME`, snapshots (made by the
  user) in `$XDG_DATA_HOME`, history and backups in `$XDG_STATE_HOME`,
  validation probes in `$XDG_RUNTIME_DIR` (the cache dir without one), each
  under `niri-center/`. Empty or relative `XDG_*` values are ignored, as the
  spec says; without a home folder nothing is written, never the working
  directory. Tests point every folder at a temp dir and never touch the
  real ones.
- **The sandbox stays on by default.** `NIRI_CENTER_NO_SANDBOX=1` is the
  escape hatch, not the baseline. Anything that needs write access outside
  the config dirs and the app's own folders must justify itself here first.
- **Scope.** Writable: the main config's dir, its symlink-resolved dir
  (saves write through the link), the app's own folders. Includes elsewhere
  stay read-only; a save touching one aborts before any write. Connect-only:
  `$NIRI_SOCKET`, the Wayland socket and the session bus: Landlock ABI v9
  gates pathname-socket connects, and without them IPC, pop-out panes and
  the file chooser portal behind Browse buttons fail. The bus adds no new
  power: the niri socket already lets the app spawn programs outside the
  sandbox.
- **When.** Applied to all threads, from `update` after iced holds its GPU
  and display handles: Landlock checks only files opened later, so applying
  in `main()` denies the GPU device and the display.

**Why.** A settings app runs with the user's rights over their dotfiles; the
kernel, not discipline, should keep it to the files it edits.

**Falsifiers.** A feature people need that cannot work inside the scope; a
write outside it that the sandbox test (`niri-center/tests/sandbox.rs`) does
not catch.
