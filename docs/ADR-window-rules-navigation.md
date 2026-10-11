# ADR: Window rules goes in, by app

**Status:** accepted 2026-10-11, tried at the user's request after weighing
the drill-in navigation of gump. Held provisionally (AGENTS.md
[provisional]).

**Decision.**

- Window rules is an index of apps (an app known by its name, so the IDs
  its packagings use are one entry; `niri-center/src/by_app.rs`), an app's
  page, and a rule.
- One selection holds at every width. With room the index stays beside the
  level open, each column scrolling on its own; in a third of the screen
  only that level shows, under a way back up ("All apps > Firefox").
  Escape or Alt+Left go up one level.
- Index rows carry no controls. An app's page shows each of its rules
  whole, with Edit, Turn off and Remove, so a change is two clicks from the
  index. A "+" in the heading of the apps list and of an app's rules makes a
  new rule.
- "Your rules" and "Rule library" are two tabs of the page; tabs between
  pages stay dropped (`ADR-design-rules.md`).
- The header search lists the page's apps and rules before settings and
  pages.

**Why.** One long list of rules beside a list of open windows made people
match the two by hand, and every row carried controls to scan past. niri
users often run the app in a half or third of the screen.

**Rejected alternatives.** Rules sorted in one list with their controls
(the previous page); a filter box inside the page beside the header
search.

**Falsifiers.** People get lost between the levels, or miss rules that
reach an app from elsewhere.
