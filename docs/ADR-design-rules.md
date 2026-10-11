# ADR: colors, copy and layout

**Status:** accepted 2026-10-04; amended 2026-10-06, 2026-10-08 and
2026-10-11. Held provisionally (AGENTS.md [provisional]).

**Decision.**

- **Colors.** The palette comes from niri's own defaults (focus ring
  `#7fc8ff`, border `#ffc87f`), dark and light, every text color at WCAG AA
  on page and surface (tested). That covers every color a widget draws:
  buttons use `theme::primary`, `secondary` and `danger` (iced's own faded
  disabled labels to half opacity; a test bans them), iced's widgets take an
  audited palette (`theme::extended`), outlines of controls clear 3:1 (WCAG
  1.4.11), and a high-contrast variant (AAA text) follows the desktop's
  portal `contrast` setting, or `prefers-contrast` in the browser. White
  cards with a hairline (and a soft shadow in light) on a grey page, a
  header band behind the search, one accent for every kind of selection,
  and an amber tint plus left bar for a changed row; the values came from a
  color review and are contrast-tested, including text on the changed and
  selected tints.
- **Copy.** Sentence case, no em dashes, no emoji or decorative glyphs,
  buttons say what happens (tested in `ui_tests.rs`). Copy carries data,
  niri's words or what a control does; it does not narrate the screen,
  point at other controls, repeat that Undo exists, or explain the app's
  reasoning. Every page opens with its title alone (the sentence under it
  restated the sidebar and the content, as GNOME and KDE do not).
- **Settings rows** are generated from the library's `settings.rs`: label,
  one-line help, the `file:line` it is written at (nothing when unset),
  control; after a change the row is amber and says what the value was.
  Unset controls show niri's own default (from niri-config's `Default`
  impls), rows whose section is off say so, and a value overridden
  elsewhere (gradient, per-output layout, a file a shell rewrites, a key
  bound again in a file niri reads later: includes count where they stand,
  and the last binding wins) says by what. Where the system knows the
  answer, offer a picker (xkb rules, cursor themes, display modes, installed
  apps, file chooser) and keep the text field as the fallback. Colors parse
  with `csscolorparser` at niri's version, so a field accepts exactly what
  niri does.
- **Navigation.** A sidebar with plain group headings and icons is the
  navigation at every width (folded to icons in a narrow window, Ctrl+B).
  Forms that add or change something open as a dialog over the page. The
  part of config.kdl a page edits (`source.rs`, read-only) shows beside it
  only when asked for (the page heading's button, Ctrl+.).
- **Layout.** The app does not rearrange itself as the window changes size;
  tabs between pages and previous/next navigation were tried and dropped as
  disorienting (one page goes in instead: `ADR-window-rules-navigation.md`).
  Where a part does change its layout (a control under its label, a list
  beside its detail), it goes by the width it gets, not the window's: the
  page's content width, worked out once per frame, or `view::by_width` for a
  small part the page cannot measure. The breakpoints are in `app::room`.
  Layouts must work in a third-width niri column (about 620 px; tested).

**Falsifiers.** People using the app report the line links as noise, or the
copy rules force worse wording than they prevent.
