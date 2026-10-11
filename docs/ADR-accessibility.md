# ADR: no AccessKit work for now

**Status:** accepted 2026-10-04. Held provisionally (AGENTS.md
[provisional]).

**Decision.** No screen-reader work for now.

**Why.** niri speaks to Orca since 25.08, but upstream iced 0.14 has no
accessibility tree; the libcosmic and plushie-iced forks do.

**Falsifiers.** Upstream iced ships AccessKit, or someone asks for
screen-reader use.
