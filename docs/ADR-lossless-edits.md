# ADR: edits splice the original text

**Status:** accepted. Held provisionally (AGENTS.md [provisional]).

**Decision.** Every edit splices the original source; nothing may reformat or
re-serialize. Any change that could rewrite a comment or a third-party block
is a bug, not an enhancement. Round-trip tests
(`niri-center/tests/roundtrip.rs`) are the gate.

- The one rewrite: a window rule saved from the form is written out anew, so
  the form opens only rules it can say in full (one `match` line, offered
  properties, no comments); every other rule is edited as text.
- Edits land where niri reads them. Includes are positional and most
  sections merge field by field, so the last occurrence in reading order is
  the one niri applies; edits go to that occurrence, in whichever file holds
  it, and missing blocks are created.
- `niri validate -c` is the dialect oracle. The parser targets niri's KDL
  dialect, not the generic KDL spec. When in doubt, run the real binary.
- Parsing is the hand-written parser in `niri-center/src/kdl`. The only
  regexes are niri's own `match` patterns, kept as data and run only to show
  which rules reach an open window, with the `regex` crate niri itself uses,
  compiled once as a rule's lines are read.

**Why.** People keep hand-written configs, shell-generated blocks (Noctalia,
DankMaterialShell) and comments in the same files; a settings app that
re-serializes them destroys what it does not manage.

**Falsifiers.** A round-trip test that fails on a real config; niri accepting
a config the parser rejects, or the reverse.
