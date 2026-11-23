# driftwall designed-contract review rubric

You review ONE project's designed architecture contract: `docs/architecture.mmd`, written for the
project, enforced by driftwall. It is a design, not an inventory: a small set of components with
responsibilities, grouped into layers, whose arrows are the dependencies the design ALLOWS. Code is
mapped to components by `%% map <node>: <glob>` lines inside the contract (no `.arch` files).
`driftwall lint --contract docs/architecture.mmd` then reports every place the code breaks the
design.

Inputs: the project worktree named in your brief, the contract and draft notes there, the project's
own docs (README, CLAUDE.md, AGENTS.md, docs/, ADRs, lint configs such as eslint zones), and the
pinned driftwall binary (`lint`, `brief`, `neighbors`, `where`, all with
`--contract docs/architecture.mmd`). Read-only: do not edit anything, do not commit or push.

Judge each criterion with evidence you checked in the code and docs (file:line):

1. Components: 4 to 15 nodes for a typical project (justify more for a large one). Each is a real
   architectural unit with one responsibility stated in its label (`id: responsibility`), not a
   file path. No component holds most of the code unless the project truly is one unit. No
   trivial components (a config file, an empty package).
2. Mapping: every source file is mapped (lint reports no `unmapped-file`, or each is intended and
   explained); globs are directory-shaped and readable, not file lists; `where` on 10 files spread
   across the tree gives the component a maintainer would expect.
3. Layers: subgraphs express the project's real layering or boundaries (entry, features, domain,
   infrastructure, shared, tooling, tests...), consistent with what the project's own docs and
   lint configs say. Read left to right, the diagram tells the story of the system.
4. Rules: the arrows are the intended dependencies. Each declared edge is either realised in code
   or deliberately planned (dotted). Every dependency the code has that the design does NOT allow
   is reported by lint as a violation, and each such violation is a genuine design problem
   (upward dependency, cycle, layer skip, leak) that a maintainer would agree should not exist,
   not an artefact of a badly drawn design. A design that simply declares every observed edge,
   cycles included, fails this criterion. An edge the contract or notes call debt must not be
   declared: debt is left undeclared so lint reports it, and listed in a forbidden-directions
   comment. A cycle hidden by putting both sides in one component fails too, unless the docs say
   the files are one unit. Test-only crossings use `-->|test|` edges, never solid ones. Rules the
   tool cannot check (type-only, package bans, file writes) go in a "not expressible yet" note,
   never under "lint reports them".
5. Fidelity: the design matches what the project's own documentation says the architecture is.
   Where docs and code disagree, the contract follows the docs' intent and lint shows the code
   gap; the notes say so.
6. Usefulness: a new engineer or coding agent reading only the contract understands the system and
   knows which dependencies are allowed. Name anything missing or misleading.

Classify each finding as DESIGN DEFECT (the contract is wrong or weak; say how to fix it),
DRIFTWALL DEFECT (the tool prevents expressing or checking the design; minimal repro), or
PROJECT OBSERVATION (a true architectural problem in the code, i.e. a lint violation worth fixing).

Verdict: SATISFACTORY only if 1 to 6 all pass. Output: verdict; blocking defects first; then
non-blocking; then the list of lint violations with your judgement of each (genuine / artefact).
