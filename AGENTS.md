# The Edit Epistemology

*Claims are held provisionally. A passing suite certifies agreement. A proof certifies within a model. World-claims stay empirical and human-anchored.*

**What this is.** A standing policy for editing this codebase, with or without model assistance, about how claims may be justified. It is non-executable guidance held in `AGENTS.md`; the evidence behind each rule (verbatim quotations, effect sizes, and the evidence against) lives in `GROUNDING.md`, whose quotations are re-checked against their sources. It is not exempt from itself: every rule names the observation that would revise it. Rules are cited by their bracketed tag (`AGENTS.md [tiers]`), never by position, so they can be added, removed or reordered without breaking a reference.

**Adopting it.** The rules are portable; the examples are Rust; substitute your language's equivalents. Everything here is provisional: run it on a real repo and revise what makes things worse.

**The receiver model (working hypothesis).** Every token in an agent's context is evidence it conditions on, about the task and about the genre the text belongs to: content steers the answer, form steers the style (Xie et al. 2021; Min et al. 2022). Artifacts are therefore chosen for what they do to their *actual receiver*. People put detail where the receiver must tell things apart and leave it out where they need not (Fan et al. 2020), and abstract conventions transfer only to receivers who share the history they grew in (Hawkins et al. 2023). A fresh agent session shares none of it: abstraction reaches it only when grounded in the artifact itself (names, types, signatures), never in shorthand or accumulated notes. The cognitive-science results are about human receivers; carrying them to code agents is analogy, held as a conjecture. *Revise if:* on this repo, stripping restating comments does not reduce the restating comments an agent then writes.

---

## Terminology

- **passing / failing**: a passing suite means the executable claims (code + tests + premises) currently agree; a failing check means at least one premise in that conjunction is false; the check does not decide which one. Colours and bare glyphs are not used; status is carried in words.
- **tier**: how independently a claim was derived, and how much of the input space it covers.
- **conjunction**: the bundle under test at once: implementation, expectation, spec-reading, environment, model.
- **provisional**: surviving attempted disproof, not confirmed.
- **agreement**: the only thing a pass certifies.
- **model**: the frame a proof lives in (itself provisional).
- **anchor**: the external, human-held reference (fallible; the only world-truth tier).
- **residue**: the non-derivable remainder worth writing: citations, genuine *why*, interface contracts.
- **canary**: a residue-comment that can catch the source drifting before tests do.
- **brain-twin**: evidence derived from the same path it claims to check: the same implementation, the same reading of the spec, or the same author or model grading its own work.
- **falsifier**: the observation, stated in advance, that revises a claim or rule.
- **jury**: whatever decides that a falsifier fired: a mechanical measurement or a reviewer other than the claim's author.
- **working memory**: notes an agent writes for its own later use.
- **teaching target**: the intent/requirement/decision that the checks corroborate; the code is derived from it, so a wrong target poisons the code.

---

## Rules

**[maps] Don't write what the code already says; measure the rest.**
A document that restates what the import graph + signatures already encode (an architecture overview, a module map, "how it works", a context dump) costs an agent without helping it and drifts for a human; reachability is not correctness, and unexercised code is compilable guesswork: the fix is more checks, not more prose. A map earns its place only by recording what the code cannot yield (a module's hidden design decision, or where to start in an otherwise undocumented area), and then it is owned and checked (CI asserts that every path it names exists). *Excluded by design:* intent and decision artifacts (RFCs, requirements, decision records, citations, acceptance criteria): the teaching target, not the map. *Revise if:* an A/B on this repo shows a redundant map improves agent success or cost.

*Grounds:* Gloaguen et al. 2026 (for, and against a blanket ban); Parnas & Clements 1986; Huey et al. 2023 (see `GROUNDING.md`).

**[tiers] Believe by execution; position every claim by tier.**
No claim about behaviour outranks its tier:
- **(a) independent derivation**: a genuinely different path that must agree: differential testing between two implementations, a deliberately naive reference, a recomputation by another route. Independence is itself a claim: state what differs (author, model, algorithm, spec reading). Independent versions still fail together where the spec is hard; one model writing both sides is a partial brain-twin.
- **(b) invariant or proof within an explicit model**: domain-stated properties remote from the implementation; machine-checked proofs or model checking. True for every input *in the model*; the model is provisional.
- **(c) fixture / pre-recorded output**: certifies today, not intent.

(a) samples inputs; (b) covers them within its model. Weigh a check by what it is independent *of* and what it *covers*, not by its letter alone. A passing suite certifies agreement, not truth. A failing check falsifies the conjunction: identify which premise to blame; never assume it's the code. Blame in order of evidence affordability: the cheapest decisive re-derivation of the expected output comes first; this is a heuristic, since the check itself cannot say which premise failed. A passing witness certifies the subspace it sweeps; state that subspace (domains, shapes, holes) in the test, and prefer property-based sweeps that can exceed it. *Revise if:* mutation testing shows this repo's tier-(a) checks kill fewer injected faults than tier-(b) checks over the same code.

*Grounds:* Duhem 1954; Quine 1951; Dijkstra; Knight & Leveson 1986 (against unconditional independence); Ron et al. 2026; Kuhn, Lakatos, Putnam (why blame-order is a heuristic) (see `GROUNDING.md`).

**[falsify] Falsify hard, and never grade your own test.**
A falsifier counts only if it was stated *beforehand* and *agreed*: set before the result is in, and not by the party whose claim is judged alone; otherwise every outcome can be read as a win. So: write checks from the spec before or apart from the implementation; test the tests, since a check that still passes when the code is deliberately broken certifies nothing (mutation testing); prefer the check that most reduces a wrong hypothesis per token: adversarial independence over redundant confirmation. Guard the inverse failure: a greedily-expanded suite starts pinning implementation quirks instead of intent. Never accept a brain-twin test as evidence. *Revise if:* mutation scores show suites held to this rule catch no more injected faults than the checks they replaced.

*Grounds:* Popper 1963, ch. 1 and n. 3; Feynman 1974 (see `GROUNDING.md`).

**[names] Names carry what; comments carry what nothing else can.**
This governs *every* name the language exposes (functions and methods, types and traits, modules, constants, parameters, closures) and the cheapest of all: local and loop variables, generic type parameters, lifetimes. Renaming is refactoring-enforced and cheap (the compiler rewrites every use; comments don't), so specificity is pushed as far down as the scope allows. A name is also the one abstraction a fresh session can read without shared history; shorthand that depends on remembered context does not transfer.

*Rust examples (illustrative, not a specification of every name):*

```diff
- // how far the drag moved horizontally, in cm
- let d = x2 - x1;
+ let drag_dx_cm = end_x_cm - start_x_cm;
```

```diff
- for (i, p) in plates.iter().enumerate() {
-     if i > 0 { total += p.w_cm; }
- }
+ for (row_index, plate) in plates.iter().enumerate() {
+     if row_index > 0 { total += plate.w_cm; }
+ }
```

```diff
- fn first_vertex<'a, T>(ring: &'a [T]) -> &'a T { &ring[0] }
+ fn first_vertex<'ring, Vertex>(ring: &'ring [Vertex]) -> &'ring Vertex { &ring[0] }
```

A descriptive name also works as a scope key: searching for a concept, a specific binding yields a precise hit-list; a generic `i` or `d` returns every loop or delta in the codebase, each needing further reading to disambiguate. A name can still overreach into a sentence; then a short *why*-comment beats the longer identifier. No *style* is prescribed (casing, prefixes and length are tooling's job); the claim is only that a name carries as much of the *what* as its scope permits. *Revise if:* review finds that descriptive names here keep needing an adjacent comment to be understood.

*Grounds:* Hawkins et al. 2023; Fan et al. 2020 (see `GROUNDING.md`).

**[residue] Comment only the residue: the residue is the canary.**
Three allowed kinds: citations and provenance, tagged and greppable, verbatim when quoting; a genuine *why*, terse, adjacent, deleted when obsolete; and interface contracts a name cannot carry (preconditions, error semantics, cost, concurrency), stating the promise, not restating the code. Every comment can drift, so keeping only the residue keeps the drift surface small; the canary role is why residue isn't decorative: it can flag the source the moment it drifts, before tests do. The *why* is also what keeps later deletion cheap: an instruction or comment whose reason is gone can only be removed at the risk of an unknown regression, so without it prose only grows. *Revise if:* restating comments are shown to lower bug-introduction here relative to residue-only comments.

*Grounds:* Ousterhout; Parnas & Clements 1986; Radmanesh et al. 2024; Chakrabarti 2026 (see `GROUNDING.md`).

**[achievability] Provable ≠ advisory; both can be wrong.**
Rules are tagged constants + computed *achievability* + ok or warning with a reason string. A hard ok not justified by achievability (a rule reporting success while its own precondition is false), or a warning without its reason, is a bug.

**[nuance] Nuance is a flag, not a verdict.**
The machine proves its own model and reports the residue; the human arbitrates intent. The tool *augments* that arbitration; it does not replace it.

**[anchor] The external anchor is the least-provisional thing you have, and it is still provisional.**
Citations and human review are the only world-truth tier; no compiler, test, or proof certifies whether the predicates themselves are right. Verbatim quoting exists because even the anchor drifts and gets the same treatment. A citation that cannot be found is drift, not anchor; search for it before it earns trust. Check that the source *endorses* the claim, not merely that the words exist: a line lifted from a position its author argues against is inverted, not anchored. Cite the evidence that cuts against a claim beside the evidence for it; a citation that could not have come out against its claim is decoration.

**[boundaries] Structure = boundaries; size is a symptom.**
A module is the unit of one kind of change plus a unit you can reason about alone from its signature and imports. Split when a change makes you read unrelated domains together; keep a long file that is one coherent algorithm.

**[mismatch] Mismatch is a re-derivation prompt, not a verdict.**
Source, test, note, or citation disagree ⇒ at least one is wrong; nobody wins by rank, except intent documents on the semantics axis. When code and a requirement disagree, the requirement is the reference: the code is the candidate drift, unless a decision record shows intent superseded the requirement. Otherwise break ties by which half is cheaper to check and better anchored externally. Requirements are themselves provisional; decision records are their revision history.

*Grounds:* Naur 1985 (see `GROUNDING.md`).

**[prose] Rate-limit prose; make the few words that survive work.**
Every non-executable sentence is context the next reader conditions on, and it can drift. Allowed words: citations/provenance, genuine *why*, interface contracts, intent and decisions (RFCs, requirements, decision records), and this policy, which carries its own falsifiers like everything else. The target is pragmatic, not minimal: detail goes where the receiver must tell plausible readings apart, and nowhere else. *Revise if:* the prose-growth metric ([working-memory]) rises while defect and review outcomes stay flat or improve.

*Grounds:* Fan et al. 2020; Liu et al. 2023; Shi et al. 2023; Chroma 2025; Chakrabarti 2026 (see `GROUNDING.md`).

**[provisional] This set is itself held provisionally, and has a jury.**
Each rule names its falsifier. A falsifier fires on a mechanical measurement (CI metric, A/B run, mutation score) or on review by someone other than the rule's author, never on the author's own reading. The first project that falsifies a rule revises the rule, not the record. The supreme rule (no rule may protect a statement against falsification) is itself a decision, so it stays open to criticism too. A rule that makes itself unrevisable is the one thing this epistemology forbids, this one included.

*Grounds:* Popper 1959 §§11, 30; Kuhn; Lakatos 1970; Putnam 1974; Miller 1974 and Tichý 1974 (see `GROUNDING.md`).

**[stack] Never weaken the falsification stack.**
Removing a verification channel requires a successor that is already green and at least as independent, covering the same claims. A transition that reduces falsification strength is a bug, not a migration step: the old half leaves only when the new half stands alone.

**[working-memory] Working memory is not a project artifact.**
Notes an agent writes for itself (progress logs, "learned" lists, context dumps) save the writing session and tax every later one: they are written for a reader who shares the writer's history and read by one who does not, and each one is also a demonstration that this is a place where notes accumulate. Such notes live outside the tracked tree and expire with the task. What deserves to outlive the task enters as a decision record or a residue comment, reviewed by a human, with its *why*. *Jury:* CI reports non-code prose added per commit. *Revise if:* a task sequence run with notes retained vs expired shows no loss in later success or cost.

*Grounds:* Chakrabarti 2026; Hawkins et al. 2023; Min et al. 2022 (see `GROUNDING.md`).

---

## Registry: keep and ban

*Test for Keep: answers "what must be / why / where is this anchored", is not re-derivable from the code, and cannot be falsified by reading the source alone.*

**Keep (intent, cause, anchor: the teaching target)**
| Artifact | Carries |
|---|---|
| RFCs / requirements | what the system *must do*: the target the tests corroborate; the code derives from it, so a wrong requirement poisons it |
| Decision records | why a choice was made; alternatives; when intent superseded a requirement |
| Citations / provenance | external truth, verbatim, greppable, with the evidence against |
| Interface contracts | the promise a name cannot carry: preconditions, errors, cost, concurrency |
| Test vectors / acceptance criteria | the executable form of intent |
| The edit policy itself | how claims are justified here, with its own falsifiers |

**Ban, unusable (re-derivable, drift-prone):** redundant structure maps, context maps / model path dumps, answer keys / glossaries of the code, code-changelogs in comments, agent working-memory notes in the tracked tree.

**Ban, adversarial (actively misleading):** what-comments, future-maintainer essays, brain-twin tests, self-graded falsifiers, golden fixtures treated as truth, overfitted suites pinning implementation quirks, dogmatic comments asserting invariants with no check, hard verdicts on judgement calls, citations inverted against their source, and any artifact that immunizes itself.

---

## Closing

Derive structure, preserve intent, falsify everything, this included. Choose every artifact for what it does to its *actual receiver*. Four moves, everywhere: give the least redundant evidence, keep the target external, let someone other than the claimant judge, and let the human hold the anchor.

*Provisional. Revise, don't defend.*
