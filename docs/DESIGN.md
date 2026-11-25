# driftwall design

driftwall enforces that `architecture.mmd` is the source of truth for a codebase. It extracts the
real import graph, compares it with the diagram, and reports every disagreement with a file and
line an agent can act on.

## Contract format

`architecture.mmd` is a strict subset of Mermaid `flowchart`. Anything outside the subset is a
hard error with a line number. Silently ignoring syntax would let a typo become a missing edge.

```
%% Comment lines start with %% and must be on their own line (no trailing comments).
%% Direction is LR, RL, TB or BT and must be the first statement.
flowchart LR
  subgraph shared["Shared"]
    model["model: ids, edges"]
  end
  %% node: id["label"] or a bare id. Boundaries group nodes and may nest, see "Nested boundaries".
  %% An edge may name one.
  %% Planned edge, tolerated until built:
  lint -.-> model
  %% Implemented edge, must be backed by code:
  cli --> lint
  %% Optional label, ignored by the linter (except `test`, see below):
  cli -->|label| report
  %% Test-only edge: allows only test imports, see "Test edges":
  lint -->|test| model
  %% Type edge: allows only type-only imports, see "Type edges":
  cli -->|type| report
  %% Boundary edge: an endpoint may be a subgraph id, see "Boundary edges":
  entry --> shared
```

- Node ids match `[A-Za-z][A-Za-z0-9_-]*`. Labels are display only.
- Ids are one namespace, as in Mermaid, where a node and a subgraph with one id are the same
  thing. A node id and a boundary (subgraph) id must therefore differ, whichever is declared
  first, and two subgraphs cannot share an id either (as two nodes cannot). Each is a contract
  error at the line of the second declaration, naming the first: `line 3: `shared` is already
  declared as a boundary on line 2` (or `as a node`).
- Edge endpoints must be declared nodes or boundaries (`subgraph` ids), see "Boundary edges". Chained
  edges (`a --> b --> c`) and `&` fan-out are errors.
- `-->` is `EdgeKind::Implemented`. `-.->` is `EdgeKind::Planned`. With the label `test`
  (`-->|test|`, `-.->|test|`) they are `EdgeKind::Test` and `EdgeKind::PlannedTest`, see "Test edges".
- With the label `type` (`-->|type|`, `-.->|type|`) they are `EdgeKind::Type` and
  `EdgeKind::PlannedType`, see "Type edges". The label `type, test` (either order, spaces around the
  comma optional) is both: `EdgeKind::TypeTest` and `EdgeKind::PlannedTypeTest`. Each word is matched
  after trimming spaces and exactly, and a label is meaningful only when every comma-separated word
  is `type` or `test`; a word twice (`type, type`) is a contract error, not a plain label.
- Any other label is display only and ignored, as before (`Test`, `tests` and `type, helper` are
  plain labels). The Mermaid form with the text inside the
  arrow (`a -- test --> b`) is not supported and stays a contract error like any syntax outside the
  subset: there is one spelling of a label, so there is one spelling of a test or type edge.
- A pair of nodes has at most one edge, whatever the kinds: `a --> b` with `a -->|test| b` is
  the existing duplicate-edge error. A solid edge already allows test imports, so the test edge would
  be redundant, and a test edge beside a solid one would be contradictory; an error says it clearly
  and a warning would let the contract mean two things.
- Edges between nodes in different boundaries are normal. A boundary may carry an edge, which is
  shorthand for the node edges between its members, see "Boundary edges".
- A comment whose first word is `exports` is the surface directive (see "Symbols and public
  surfaces"), and one whose first word is `map` is the mapping directive (see "Contract map").
  The comments whose first word is `order:` or `order open:` (see "Layer order"), `isolated:` (see
  "Isolated boundaries"), `each` (see "Edge templates"), `no-cycles` or `no-cycles:` (see "No cycles"), `budget` (see "Budgets") `tests:` (see "Test isolation") or `facade` (see "Re-exports") are the rule directives
  of "Contract rules". The colon must follow the word directly, so a comment such as `%% tests -->
  ...` stays plain. The comments whose first word is `packages` or `forbid-packages` are package rules (see
  "Package rules"). Every other comment is plain.

### Test edges

A test edge, `a -->|test| b`, allows the dependency of `a` on `b` from test code only. Without
it a contract cannot allow a parity test or a fixture to cross a boundary without also permitting
production code to do so. It uses the `test_only` flag of every import site (see "Import kind").

- A test-only site on the pair is allowed, like on a solid edge.
- A site that is not test-only, on a pair whose only edge is a test edge, is an `undeclared-edge`
  error. No new kind: the production dependency really is undeclared, so the finding and its hint
  (remove the import, or ask to approve the edge) are those of any undeclared edge, and tools that
  know the rule keep working. The message differs: "... but architecture.mmd declares only a test
  edge a -->|test| b, which allows test imports only". Each production site is one finding.
- A solid test edge with no site at all is `stale-edge`, exactly like a solid edge (the message
  names the arrow `-->|test|`). Production sites do not realise it: a test edge with production
  sites only is stale as well as each site being an error.
- A solid edge whose every site is test-only is `test-only-edge`, a warning: "edge a --> b is
  realised only by test code; declare it `-->|test|`". A solid edge with no site is stale instead.
  `--strict` fails on it like on any warning. driftwall's own contract has no such edge.
- A dotted test edge, `a -.->|test| b`, is the planned form: tolerated while no code uses it,
  allowing test sites only. When test sites realise it, it is `planned-realized` ("promote it to
  `-->|test|`"). Production sites on it are `undeclared-edge` like above.
- A dotted edge with no label, `a -.-> b`, is realised only by production code. A site written in
  test code does not realise it: the app does not use the dependency, a test does. When every site
  of the pair is test-only the finding is not `planned-realized` (which would say "make it solid")
  but `test-only-edge`, a warning: "planned edge a -.-> b is used only by test code; the app does
  not use it yet; declare it `-.->|test|` or `-->|test|` if that is intended". The hint says the
  same. Test sites on the dotted edge are still allowed, as on any dotted edge, and one production
  site makes it `planned-realized` as before. A dotted boundary edge is judged the same way over all the pairs it covers: only test sites under it is `test-only-edge`, one production site under it is `planned-realized`. A planned `|test|` edge, and a planned `|type|` edge,
  keep their rules above and below.

`brief` and `neighbors` draw a test edge `a -->|test| b` and a dotted one `a -.->|test| b`. The
`brief` summary line counts `implemented`, `planned` and, only when there is one, `test` edges (a
dotted test edge counts as planned). In JSON the edge `kind` is `implemented`, `planned`, `test` or
`planned-test`, the same names `diff` uses: its text writes a changed or listed test edge as
`a -->|test| b (test)`, and a change between kinds is a change, like planned to implemented. An
edge code uses on a pair with only a test edge keeps the declared kind in `brief`; each
production site shows as one `undeclared-edge` finding in `lint`.

### Type edges

A type edge, `a -->|type| b` (planned: `a -.->|type| b`), allows the dependency of `a` on `b` through
type-only import sites only: a node may use another's types and not its runtime code. It uses the
`kind` of every import site (see "Import kind") and mirrors "Test edges", and the findings are the
same ones with the same hints.

- A site of kind `type-only` on the pair is allowed. That is what the import kind rules classify:
  TypeScript and JavaScript `import type`, `export type ... from`, all-`type` specifier lists,
  imports in declaration files, `import("x")` in a type position, JSDoc type imports, and Python
  imports inside `if TYPE_CHECKING:` (or `if typing.TYPE_CHECKING:`), which the import kind rules
  already classify as type-only. Rust and Swift have no type-only import, so a type edge between
  nodes written in them is never realised: every site is a finding and the edge is stale. The same
  holds for shell, embeds and assets: their sites are `runtime`, `embed` or `asset`, not `type-only`.
- Any other site on a pair whose only edge is a type edge (kind `runtime`, `embed` or `asset`) is an
  `undeclared-edge` error, one finding per site, with no new kind. The message says it plainly: "...
  but architecture.mmd declares only a type edge a -->|type| b, which allows type-only imports
  only".
- A solid type edge with no type-only site is `stale-edge` (the message names the arrow
  `-->|type|`): runtime sites do not realise it, so a type edge with runtime sites only is stale as
  well as each site being an error. A dotted type edge is tolerated without sites; when a type-only
  site realises it, it is `planned-realized` ("promote it to `-->|type|`").
- `type` and `test` are independent restrictions on the site, and the label `type, test` allows the
  union: a site is allowed when it is type-only or test-only. In practice, production code may
  import the target's types, and test code may import anything from it. `-->|type|` alone does not
  let tests import runtime code (`a.test.ts` importing `b` at runtime is an `undeclared-edge`), and
  `-->|test|` alone does not let production import types. A site that is test-only and type-only is
  allowed on all three labels. `type, test` is stale when no site is type-only or test-only, and a
  `test-only-edge` warning never applies to it. The intersection ("type-only and test-only") is not
  a label: it adds nothing a `test` edge does not allow in practice.
- A solid edge realised only by type imports is not a warning: whether it should be `-->|type|` is a
  design choice, so driftwall stays quiet (the warning would force every design). `brief` and
  `neighbors` mark such an edge instead, see below. `--strict` is unaffected.

`brief` and `neighbors` draw a type edge `a -->|type| b`, `a -.->|type| b` and `a -->|type, test| b`.
The `brief` summary line counts `implemented`, `planned` and, only when there is one, `test` and
`type` edges: a dotted type or test edge counts as planned, and a `type, test` edge as `type`. In
JSON the edge `kind` is `type`, `planned-type`, `type-test` or `planned-type-test` besides the four
existing names, and `diff` writes them the same way (`a -->|type| b (type)`). An edge declared
`-->` (solid, unrestricted) whose every site is type-only, and which has at least one site, is marked
`could be -->|type|` after its annotations in `brief` and `neighbors` text, and has
`"type_candidate": true` in JSON (absent otherwise), so a designer can choose to tighten it.

### Nested boundaries

A `subgraph` may contain another `subgraph`, to any depth, each closed by its own `end`:

```
flowchart LR
  subgraph features["Features"]
    subgraph billing["Billing"]
      billing-controller
      billing-service
    end
    subgraph search["Search"]
      search-service
    end
    features-shell
  end
```

- Membership is transitive. A node belongs to the boundary it is written in and to every boundary
  around that one: `billing-service` is a member of `billing` and of `features`. The members of a
  boundary are the nodes declared between its `subgraph` line and its `end`, at any depth. A boundary
  with no node at any depth has no members (an edge to it is a contract error, see "Boundary edges").
- Ids are one namespace across nodes and boundaries at every level: the id rules of "Contract
  format" apply to a boundary however deep it sits, and a boundary cannot reuse the id of one that
  contains it or of one beside it.
- Every rule that names a boundary uses its members: boundary edges, `%% order`, `%% no-cycles:
  boundaries`, `%% isolated` and the `via` and `redundant-edge` findings. Each section says how
  nesting enters it.
- Malformed nesting is a contract error at its line: an `end` with no open `subgraph` (`line 9:
  `end` without an open subgraph`), and a `subgraph` still open at the end of the file, reported at the
  line of the innermost one that is open (`line 3: subgraph `billing` is never closed with `end``).
  What `end` closes is the innermost open `subgraph`, as in Mermaid.
- Only the innermost boundary of a node is kept as its `boundary`; the ones around it are its outer
  boundaries. `render` writes the nesting back, so a contract round-trips. Boundary labels are still
  not stored (a rendered boundary is labelled with its id), and a boundary with no node at any depth
  is not rendered, as before.
- Mermaid draws nested subgraphs, so the diagram still renders (checked with `mermaid-cli` in headless
  Chrome: three levels, boundary edges between inner and outer boundaries, one cluster per boundary).
- `init` drafts have one level and no nesting.

### Boundary edges

An edge endpoint may be a boundary (a `subgraph` id) as well as a node: `a --> b`, `a --> sub`,
`sub --> a` and `sub --> sub` are all edges, with every arrow and label of the other edges (`-->`,
`-.->`, `|test|`, `|type|`, `|type, test|`). A node and a boundary never share an id, so an endpoint
is never ambiguous. Mermaid draws these edges between the subgraph boxes, so the diagram still
renders (checked with Mermaid 12, including a boundary to itself and a node to its own boundary).

A boundary edge is shorthand: it declares an edge of its own kind for every pair it covers.

- Covered pairs. A side is the node itself, or every node of the boundary. The edge `S --> T` covers
  every ordered pair `(x, y)` with `x` in the side `S`, `y` in the side `T`, and `x` different from
  `y`: an import inside one node is never an edge, so no pair has the same node on both sides.
- Members. The members of a boundary are the nodes declared inside it at any depth (see "Nested
  boundaries"). A node is written in exactly one boundary and is a member of it and of every boundary
  around it. A node in no boundary is reachable only as a node endpoint. An edge from or to an inner
  boundary covers only the nodes of that inner boundary: `billing --> shared` says nothing about
  `search`, while `features --> shared` covers `billing` and `search` too.
- Nested ends. The two ends of a boundary edge may be the same boundary, or one may contain the other
  (`features --> billing`, `billing --> features`, `features --> features`). That is allowed and means
  what the covered pairs say: `features --> billing` is every member of `features` using every member of
  `billing`, other than a node using itself, including the members of `billing` using each other. The
  exception is `%% isolated`, which refuses such an arrow when it covers a pair between two children
  of the isolated boundary, see "Isolated boundaries".
- A boundary to itself, `features --> features`, means the members may depend on each other: it
  covers every ordered pair of distinct members. Without it, imports between two nodes of one
  boundary need node edges like any others: a boundary is a grouping and grants nothing by itself.
  A node to its own boundary, `a --> features` with `a` in `features`, covers `a` to every other
  member, and `features --> a` every other member to `a`.
- A boundary endpoint with no nodes is a contract error: the edge would cover nothing, and a typo
  must not become a silent no-op. An edge that covers no pair for another reason (a boundary with
  one node to itself) is legal and, being unrealised, stale like any edge.
- A pair of endpoints has at most one edge, as for nodes: `features --> core` twice, or with two
  different arrows, is the duplicate-edge error naming both lines. Edges with different endpoints
  may overlap freely: `features --> core` beside `a --> core` (with `a` in `features`), or beside a
  node edge between two of the nodes it covers.
- A node edge is identified by its two nodes, and a boundary edge also keeps its line, because
  findings and `brief` name it.

Lint treats the covered pairs as declared, with these rules.

- Allowed. A dependency from `x` to `y` is allowed when some edge covers `(x, y)`, node edge or
  boundary edge, and the kind of that edge allows the site, by the rules of "Test edges" and "Type
  edges": each covering edge is judged alone, and a site is allowed when any covering edge allows it.
  Nothing covers the pair: `undeclared-edge` exactly as before. Some edge covers it and none allows
  the site: `undeclared-edge` with the restricted-edge message of "Test edges" or "Type edges", which
  names the winning cover (below), `features -->|test| core` (line 31) when it is a boundary edge.
- Precedence. When several edges cover one pair, the most permissive wins, in this fixed order:
  unrestricted (`-->`, `-.->`), then `type, test`, then `type`, then `test`; at equal restriction a
  solid edge before a dotted one, a node edge before a boundary edge, then the lower contract line.
  The winner is the edge that `brief` and `neighbors` name for the pair (`via`, below) and that the
  restricted-edge message names. It never changes whether a site is allowed (any covering edge that
  allows it is enough), and never changes staleness (each edge is judged alone, below). A solid node
  edge beside a `-->|test|` boundary edge on one pair: the solid edge wins.
- Stale. A boundary edge is stale (`stale-edge`) only when none of the pairs it covers is realised
  by a site the edge allows (the test of its kind in "Test edges" and "Type edges", over all its
  pairs). One realised pair is enough, so a boundary edge is not stale because some members do not
  use it. The other findings on an edge follow its kind in the same way, over all pairs: a dotted
  boundary edge is `planned-realized` once any covered pair is realised by a site it allows (the same
  meaning as for a node edge of that kind, so a test-only site realises a dotted unrestricted edge
  here as it does there), and a solid unrestricted one is `test-only-edge` when every site on its
  pairs is test-only. The messages name the boundary arrow (`edge features --> core is declared but
  no import realises it`).
- Redundant. A node edge `a --> b` that a boundary edge covers, with a kind at least as wide, is the
  warning `redundant-edge`, without a source location like `stale-edge`, naming the covering edge
  and its line: "edge a --> b is redundant: features --> core (line 31) already covers it". At least
  as wide means the boundary edge allows every site the node edge does (unrestricted covers any
  kind, `type, test` covers `type`, `test` and itself, any kind covers itself), and it is solid
  unless the node edge is dotted (a dotted boundary edge does not require the dependency to be
  built, so a solid node edge under it is not redundant). A narrower cover is not redundant: a solid
  node edge beside a `-->|test|` boundary edge is needed. It is a warning because the contract still
  means one thing, only longer than needed; `--strict` fails on it. The edge named is the first wide
  enough one in the precedence order. A boundary edge covered by another boundary edge is not
  reported.
- Layer order and no-cycles treat a boundary edge as its pairs, see "Layer order" and "No cycles".
- `init` is unchanged: it writes node edges only.

`brief` and `neighbors` show, for each realised pair, the edge that allows it. A pair a node edge
declares has nothing extra. A pair a boundary edge allows is annotated `via features --> core, line
31` after its import count, and a pair no edge allows stays `UNDECLARED`. In JSON an edge has `via`
(`{"edge": "features --> core", "line": 31}`, absent for a node edge or an undeclared pair) and its
`kind` is the winning edge's. The `brief` summary line counts a boundary edge once, as itself, and not the pairs it allows.
`brief` lists the boundary edges in a `boundary edges:` block before
the `rules:` block, one per line with the arrow, the line, how many pairs it covers and how many of
them code realises (`features --> core (line 31): 6 of 24 pairs realised`); in JSON they are
`boundary_edges` (`from`, `to`, `kind`, `status`, `line`, `covers`, `realised`), absent when there are none.
`diff` treats a boundary edge like a node edge, keyed by its two endpoints: added, removed and
changed kind, in the same text forms as node edges (`a --> b` becomes `features --> core`), in JSON as
`added_boundary_edges`, `removed_boundary_edges` and `changed_boundary_edges` with the endpoints as
strings.

### Edge status

Every edge has one status, a function of what the contract declares for the pair and of what code
does on it. `lint` reports the status as a finding, and `brief` and `neighbors` print and serialise
the same status, from the same function (`edge_status` in the `model` module), so the two views
cannot disagree.

The declaration is one of: no edge (`none`), an edge of a kind (see "Test edges" and "Type edges"
for the kinds), or a pair an `%% order open` chain allows without an edge (`open`, see "Open
order"). Code does one of: nothing (`unseen`), or has sites on the pair. Of the sites three facts
matter: whether one is allowed by the kind (`allowed`), whether one is not test-only (`production`),
and, implied by both, that there is one. `allowed` and `production` are independent: a `|type|` edge
allows a production type-only site, a `|test|` edge a test-only one, and a runtime production site
on a `|test|` edge is production but not allowed. An unrestricted edge allows every site, so there
`production` implies `allowed`.

| Declaration | Observation | Status | Finding | Severity |
| --- | --- | --- | --- | --- |
| none | any site (an open chain that narrows the pair counts as none) | `undeclared` | `undeclared-edge`, once per site, except a site that is an `upward-edge` or an `isolation-breach`, which has only that finding | error |
| open chain allows | any site | `open` | none | |
| solid, unlabelled `-->` | unseen | `stale` | `stale-edge` | warning |
| solid, unlabelled | sites, none production | `test-only` | `test-only-edge` | warning |
| solid, unlabelled | a production site | `ok` | none | |
| solid `|test|`, `|type|`, `|type, test|` | no allowed site (unseen, or only sites the kind does not allow) | `stale` | `stale-edge` | warning |
| solid `|test|`, `|type|`, `|type, test|` | an allowed site | `ok` | none | |
| dotted, unlabelled `-.->` | unseen | `planned` | none | |
| dotted, unlabelled | sites, none production | `test-only` | `test-only-edge` (with the dotted hint) | warning |
| dotted, unlabelled | a production site | `planned-realized` | `planned-realized` | warning |
| dotted `|test|`, `|type|`, `|type, test|` | an allowed site | `planned-realized` | `planned-realized` | warning |
| dotted `|test|`, `|type|`, `|type, test|` | no allowed site | `planned` | none | |

"Declared, but only sites its kind does not allow" is not a status of its own. On a solid
restricted edge it is `stale` (the table above: a disallowed site does not realise the edge), on a
dotted one `planned`. What is distinct is the finding about each such site: an `undeclared-edge`
error whose message names the restricted edge. That finding is about a site, not about the edge, so
an edge can have it and any status. Only a pair with no edge at all has the status `undeclared`. The
status is about the declaration only: a pair whose sites are `upward-edge` or `isolation-breach`
findings (which have no `undeclared-edge`, see "Layer order" and "Isolated boundaries") is still
`undeclared` in `brief` and `neighbors`.

A boundary edge has one status over all the pairs it covers, from the union of their sites (stale
only when no covered pair is realised, see "Boundary edges"); the expansions of one `%% each` arrow
share one status, judged together (see "Edge templates"). In `brief` and `neighbors` a pair that
a boundary edge allows has the status of that boundary edge, as it is the boundary edge that `lint`
judges, not the pair.

The finding kind of each status, and the exit code that follows, are those of "Rules"; `ok`,
`open` and `planned` have no finding. In `brief` and `neighbors` text the line of an edge ends,
where `UNDECLARED` stands, with a marker for each status that has a finding: `UNDECLARED`, `STALE`,
`PLANNED-REALIZED`, `TEST-ONLY`. `ok`, `open` and `planned` have none. JSON edges (and boundary
edges) gain `"status"`, one of `ok`, `open`, `undeclared`, `stale`, `planned`, `planned-realized`,
`test-only`. It is additive: `kind` is unchanged and `null` still means undeclared.

### Contract rules

Rules that are not about one pair of nodes are directives, one `%%` line each, parsed with the
contract and stored in `Contract::rules`. Each is checked deterministically from data driftwall
already has. A directive that is malformed, names something undeclared, or is given twice (one
`%% order`, one `%% tests`, one `%% no-cycles`, one `%% no-cycles: boundaries`, one `%% budget` per
node, a node once in `%% facade`, a boundary once in `%% isolated`) is a contract error with its line, like any other syntax outside the subset: silently
ignoring one would let a typo become a missing rule. `render` writes them back, after the edges.
`brief` and `neighbors` show them, see below.

#### Layer order

`%% order: <boundary> > <boundary> > ...` lists boundaries (`subgraph` ids) from the top, the
entry points, to the bottom, the foundations. A node may depend on nodes in its own boundary, in
a lower listed boundary, or in none: it may not depend on a node in a higher listed boundary, so a
contract need not draw every arrow to forbid the wrong direction.

- Every id must be a declared `subgraph` (a node id, an undeclared id, an id listed twice in one
  line, and fewer than two ids are contract errors).
- Several chains. A contract may have several `%% order` lines, each an independent chain: a repo
  with a Node CLI stack and a Rust engine stack, or a Node program and a browser page, orders each
  stack on its own line and says nothing about how the stacks relate. A boundary may be listed in
  more than one chain only if the chains agree: for every two boundaries two chains both list, both
  put the same one above. A chain that puts them the other way is a contract error at its line
  naming the earlier line: `%% order` on line 14 contradicts the one on line 12: `b` is below `a`
  there but above it here. Only boundaries two chains share are compared: chains are not merged, so
  `a > b` and `b > c` say nothing about `a` and `c`.
- Nesting. A listed boundary stands for all its members at any depth (see "Nested boundaries"): a node
  is in the listed boundary that contains it, directly or through inner boundaries, and `%% order:
  app > features > shared` orders every node inside `features` as one layer. Inner boundaries are
  not ordered by their outer boundary's place; they are ordered only if a chain lists them. A chain may
  not list a boundary together with one nested in it (`features > billing` with `billing` inside
  `features`, or the reverse order): the node's layer would be ambiguous, so it is a contract error at
  the directive: `line 12: malformed `%% order` line: `features` contains `billing`, and a chain may
  not list both`. To order the
  inner boundaries of one boundary, list them in a chain without the outer one; two chains may list a
  boundary and one nested in it, because chains are independent (a node takes its layer, in each
  chain, from the boundary that chain lists).
- Nodes in the same listed boundary, and nodes in no listed boundary (declared outside every `subgraph`,
  or in an unlisted one) are unconstrained by this rule, in both directions. So are two nodes whose
  boundaries no one chain lists together: the check applies per chain, and a pair is constrained
  only by a chain that lists both of its boundaries.
- Every import site that crosses from a node in a lower listed boundary to a node in a higher
  one in some chain is an `upward-edge` error at the site, whatever the contract declares: an arrow
  does not excuse it. One finding per site (when chains agree, every chain listing both boundaries
  says the same, and the message names the first such chain's line). A site that is an `upward-edge` is
  never also an `undeclared-edge`, whatever the pair declares: the arrow that finding asks for is a
  contract error (below), so it would point at a fix that cannot work. The hint names fixes that work:
  remove the import, or ask the user to approve changing the `%% order` line. The message names the
  two nodes and boundaries.
- A declared arrow that points upward in some chain (dotted ones too) is a contract error at its
  line, naming the order line: the contract contradicts itself, so there is no lint to run. A boundary
  edge (see "Boundary edges") is judged on every pair it covers: if any pair goes from a lower listed
  boundary to a higher one in some chain, the edge is that error, naming its own endpoints (`edge
  `core` to `features` points upward: `core` is below `features` ...`). So `core --> features` is an
  error when a chain puts `features` above `core`, `features --> core` is fine, and a boundary edge
  inside one boundary, or between boundaries no chain lists together, is allowed. The exception is a
  `-->|test|` or `-.->|test|` edge, see below. A type edge (`|type|`, `|type, test|`) is not an
  exception: a type-only import is a dependency, and the layer order forbids it too.
- Test sites. A site that is test-only (see "Import kind") is exempt only when the pair has a test
  edge `a -->|test| b` (or `-.->|test|`), declared on the pair or covering it as a boundary edge: the
  designer then approved test code
  reaching up explicitly, one pair at a time, and the arrow is not a contract error. Without
  that edge, test code is held to the order like production code (a test of a low layer that
  drives it from the entry point must say so with a test edge). The exemption is for sites the
  edge allows: a production site on the same pair is not allowed by the edge, so it is an `upward-edge`
  (and only that, see above). A test-only `upward-edge` is the one whose fix can be an arrow, so its hint
  says: remove the import, or ask the user to approve the test edge `a -->|test| b` (the real node
  ids); with that edge declared the site is clean.

##### Open order

`%% order open: app > features > shared > core > domain` is a chain in open mode. It lists boundaries
exactly like `%% order:` and is its own directive: the word `open` sits before the colon, so a plain
`%% order:` keeps its meaning byte for byte and cannot be confused with a boundary list (`open`
after the colon would be read as a boundary id, and a boundary may be named `open`). Several chains,
the agreement check and the upward rules above apply to open chains as to plain ones, and open and
plain lines may be mixed in one contract. The reason for the mode: with seven layers that may each
use any layer below, a plain chain still needs every allowed downward dependency drawn, up to 21
layer-to-layer arrows. In an open chain they are implicit and arrows only narrow.

- Nesting. As in every chain, a node is in the boundary the chain lists around it, at any depth (see
  "Nested boundaries"): with `features` listed, a node of `features/orders` is in `features`, both for
  the allowance and for narrowing, and "inside `U`" below means any depth.
- Implicit allowance. A dependency from a node in boundary `U` to a node in a lower boundary `L` of
  the same open chain, with no arrow of any kind between them, is allowed as if a solid unlabelled
  edge covered the pair. It allows production, test, runtime and type-only sites alike. Upward stays
  forbidden: `upward-edge`, as in every chain. Two nodes of one boundary are not affected: they still
  need node edges or a boundary self-edge (`features --> features`), because the allowance is
  between layers.
- Narrowing. If the contract declares any edge from inside `U` to inside `L`, then `U`'s access to
  `L` is limited to the declared edges and the allowance no longer applies to that pair of layers.
  "Any edge" is any node edge between a node of `U` and a node of `L`, and any boundary edge one of
  whose covered pairs goes from a node of `U` to a node of `L` (`U --> L`, `a --> L`, `U --> b`),
  whatever its arrow. Membership is read with the member lookup of boundary edges. Only the pair of
  layers `(U, L)` is narrowed: `U` to another lower layer `M` has its own allowance, and a layer
  that declares no edge towards a lower layer keeps the whole allowance towards it.
  Example, chain `app > features > shared > core`. With no arrow at all, `features` may use `shared`
  and `core`. After `a1 --> c1`, with `a1` in `features` and `c1` in `core`: `a1` importing `c1` is
  allowed (the edge), a `features` node importing another `core` node `c2` is `undeclared-edge`, and
  every `features` import of a `shared` node is still allowed, since no edge runs from `features` to
  `shared`. `app` is unaffected.
- Kinds. A `-->|type|`, `-->|test|`, `-.->` or `-->|type, test|` edge narrows like a solid one,
  because any declared arrow narrows: a designer who wrote down how `features` reaches `core` has
  stated the whole list. So `a1 -->|test| c1` makes a production import `a1` to `c1` an
  `undeclared-edge` with the restricted-edge message of "Test edges", and `a1` importing `c2` is
  `undeclared-edge` too. The allowance itself is solid and unrestricted: while it applies it allows
  any site, test or production, runtime or type-only.
- Plain and open chains. A boundary listed in an open and in a plain chain gets the allowance from
  the open chains only: a plain chain neither grants nor removes it and constrains only upward
  imports, as ever. The allowance applies to a pair when some open chain lists both boundaries with
  the importer above. When two open chains qualify, the first in file order is the one `brief` names.
- Undeclared-edge message. A pair no allowance covers is `undeclared-edge` as before. When an open
  chain lists the two boundaries and narrowing has switched its allowance off, the message says so:
  "`a1` imports `c2` (...) but architecture.mmd declares no edge a1 --> c2; the `%% order open` on
  line 5 limits `features` to the edges it declares towards `core` (1 declared) and this pair is not
  among them".
- No cycles. Implicit allowances are not declared edges and are ignored: `%% no-cycles` judges
  declared edges only, and an allowance only points down, so it cannot close a cycle.
- Stale and redundant. The allowance is not a declared edge, so it is never stale, and a site it
  allows realises no declared edge. A declared arrow that the allowance would also cover is not
  `redundant-edge`: it narrows, which the implicit allowance does not do. `stale-edge` and the other
  edge findings apply to declared narrowing arrows as to any edge.
- `brief` and `neighbors`. A realised pair allowed only by an open chain is a solid edge in the
  output with `via order open: app > features > shared > core > domain, line 7` (the chain as
  written, then its line), and in JSON `via` is `{"edge": "order open: app > ...", "line": 7}` and
  `kind` is `implemented`. It is not counted among the declared edges of the summary line, as a pair
  covered by a boundary edge is not. A pair nothing allows stays `UNDECLARED`. The `rules:` block
  writes an open chain as `order open: ...`, and JSON `rules` adds `open_order`, the open chains as
  boundary lists (`order` keeps every chain, open or plain, in file order).
- `diff` reports a chain changing between plain and open as the directive line removed and added
  (see "Diff beyond the contract").
- `render` writes `%% order open: ...` back.

#### No cycles

`%% no-cycles` makes the declared edges a directed acyclic graph over nodes, and
`%% no-cycles: boundaries` makes the boundary graph one too. They are independent lines, each at
most once, and both are checks of the contract alone: code that crosses an undeclared pair is
already `undeclared-edge`, so a cycle in code can only come from a declared one.

- Nodes: a boundary edge counts as the node edges of its covered pairs (see "Boundary edges"), so a
  cycle through `features --> core` and a node edge back is found. Every declared edge counts, solid or dotted (a planned cycle is still a cycle you intend to
  build), type edges included, and test edges (`-->|test|`, `-.->|test|`) excluded. A test edge
  expresses that tests reach across the design, and the usual shape of tests is the reverse of the
  production dependency, so it would read as a cycle without being one. A `type, test` edge counts:
  its type-only production sites are dependencies.
- Boundaries: every boundary at every level is a vertex (see "Nested boundaries"). There is an edge
  from boundary `X` to boundary `Y` when a counted node edge goes from a node in `X` that is not in
  `Y` to a node in `Y` that is not in `X`. So `X` and `Y` never contain each other, and a boundary
  never has an edge to one around it or inside it: an edge from `billing` to a node of `features` that
  is not in `billing` adds nothing between `billing` and `features`, and between two features
  it adds an edge between the two feature boundaries only (not to `features`, which holds both). Edges
  inside one boundary and nodes in no boundary add nothing.
- A cycle is a contract error at the line of the directive: `line 12: `%% no-cycles` but the
  declared edges form a cycle: a --> b (line 20), b --> c (line 21), c --> a (line 22)`. A step that
  comes from a boundary edge names it and its line, `a --> b (via features --> core, line 31)`; when a
  node edge and boundary edges cover the pair the node edge is named, else the lowest line. Only one is
  listed: the shortest cycle through the smallest node id (boundary id) that is on any cycle, so
  the message is the same on every run. Fix it and run again for the next one. The boundary form
  lists boundaries without lines: `a --> b --> a`.
- A cycle inside a node (`marks` and `camera` modules of one node) is invisible by construction:
  a node is the unit, and imports within it are no edges.

#### Isolated boundaries

`%% isolated: <boundary>[, <boundary>...]` makes the children of a boundary independent of each
other. It is for a feature-module layout: `features` holds one boundary per feature and the
features must not use each other. Without it that is only the absence of arrows, and anyone who
meets the lint error can add an arrow between two features; with it the arrow is refused.

- The children of an isolated boundary are its direct children: each boundary written directly inside
  it, taken with everything nested in it, and each node written directly inside it. A child boundary
  with no node at any depth is not a child. Dependencies inside one child, however deep, are not
  affected, and neither are dependencies between a child and anything outside the isolated boundary.
- Every id must be a declared `subgraph`, each at most once in the whole contract (an id listed twice, in
  one line or two, a node id, an undeclared id, and an empty list are contract errors). Several
  boundaries may be listed, nested ones too: `features` and `billing` inside it each isolate their own
  children.
- Breach. Every import site from a node in one child to a node in a different child is an
  `isolation-breach` error at the site, whatever the contract declares: no arrow excuses it, because
  no such arrow can exist (below). Type-only, test-only, embed and asset sites are sites like any
  other. The message names the isolated boundary, the two children and the two nodes: "boundary
  `features` is isolated: `billing-service` (in `billing`) imports `search-view` (in `search`)". One finding per site, and never also
  an `undeclared-edge`, as with `upward-edge`: the arrow it asks for is a contract error. The hint:
  remove the import, or move the shared code to a node outside the isolated boundary, or ask the user
  to approve changing the `%% isolated` line. A site that is both an `isolation-breach` and an
  `upward-edge` keeps both, which are distinct rules.
- Test sites are not exempt. A test of one feature that imports another feature's code is the same
  coupling in tests, and the point of the rule is that a feature can be read, changed or removed by
  looking inside it. The shared test helper belongs in a node outside the isolated boundary. A test
  edge between children would reopen the escape hatch for one kind of site, so there is none.
- Arrows. A declared edge that covers a pair of nodes in two different children of an isolated
  boundary is a contract error at its line, node edge or boundary edge, whatever its arrow or label
  (dotted, `test`, `type` too): `line 31: edge `billing-service --> search-view` joins `billing` and
  `search`, children of `features`, which `%% isolated` on line 12 keeps apart`. A boundary edge is
  judged on its covered pairs: `billing --> search` and `features --> features` are errors,
  `features --> shared` is not (its pairs leave `features`), `billing --> billing-service` is not
  (inside one child), and `features --> billing` is an error (it covers pairs from other features into
  `billing`). The first such edge by line is reported.
- Fewer than two children is the warning `stale-isolation` ("`%% isolated` on line 12 of architecture.mmd names
  `features`, which has one child so there is nothing to keep apart"; "no child" for an empty one), without a source location, naming the line of the
  directive. `--strict` fails on it.
- `brief` and `neighbors` show the rule in the `rules:` block, `isolated: features (billing, search,
  features-shell)`, the children in id order, and JSON has `rules.isolated`, a list of `{"boundary": "features",
  "children": ["billing", "search", "features-shell"]}` in boundary id order, absent when the
  contract has none. `diff` reports the directive line and a node moving between boundaries or a change of
  nesting (see "Diff beyond the contract").

#### Edge templates

`%% each <boundary>: <arrow>[, <arrow>...]` declares the same internal arrows for every feature of a
feature-module layout, once. It is for the layout of "Isolated boundaries": `features` holds one
boundary per feature, and each feature has the same parts.

```
subgraph features["Features"]
  subgraph billing["Billing"]
    billing-controller
    billing-service
    billing-store
  end
  subgraph search["Search"]
    search-controller
    search-service
  end
end
%% each features: controller --> service, controller -.-> view, service -->|type, test| store
```

An arrow is `<role> <arrow> <role>`, where the arrow is any form of an ordinary edge, parsed by the
same code: `-->`, `-.->`, `-->|test|`, `-->|type|`, `-->|type, test|` and the dotted forms (a comma
inside the `|...|` label does not end the arrow). A role is a node id suffix, `[A-Za-z][A-Za-z0-9_-]*`.

- Children. `<boundary>` must be a declared boundary that has child boundaries (a direct child
  boundary with a node at some depth, as in "Isolated boundaries"). The template applies to each
  such child `C`. Nodes written directly in `<boundary>` are not children here, and neither are
  boundaries nested deeper: a template reaches one level.
- Roles. Inside child `C`, the role `r` is the node with the id `C-r` that is a member of `C`, at
  any depth. Nothing else resolves: a node `billing-service` that sits outside `billing` is not the
  role `service` of `billing`.
- Expansion. The arrow `a --> b` expands, for each child `C`, to the declared edge `C-a --> C-b`,
  with the kind of the arrow. If `C` has no `C-a` or no `C-b` the expansion is skipped without a
  word: a feature may have no view. An expansion is a node edge for every rule (see below), written
  nowhere: `render` writes the template back, never its expansions.
- Contract errors, each with the line of the `%% each`, exit 2: `<boundary>` not a declared
  boundary (a node id, an undeclared id) or a boundary with no child boundary; a malformed list
  (empty, an empty arrow, an arrow that is not `<role> <arrow> <role>`, a role twice the same on
  both ends, a role that is not an id); a role that resolves in no child (`role `servce` resolves in
  no child of `features`: no node `billing-servce` in `billing`, ...`), which catches a typo; an
  arrow listed twice in one line, and two expansions of one pair of nodes (two arrows or two lines
  that expand to the same edge), which is the duplicate-edge error naming both lines. A comment
  whose first word is `each` and that does not have this form is a contract error, never a plain
  comment.

Lint treats an expansion as a declared edge of its kind:

- Allowed, kinds, planned and the other findings follow "Boundary edges" and "Test edges" and "Type
  edges", with the expansion as a covering edge between two nodes. `undeclared-edge` messages name
  it as `each features: controller --> service` (line 12).
- Stale. A template arrow is stale (`stale-edge`) only when no child realises it by a site its kind
  allows: one warning for the arrow, without a source location like every `stale-edge`, naming the
  `%% each` line and the arrow, not one per child. `planned-realized` and `test-only-edge` are judged
  the same way, over all the children of the arrow.
- Redundant. An explicit node edge `billing-controller --> billing-service` that an expansion
  covers with a kind at least as wide is `redundant-edge` (a warning), naming the template: `edge
  billing-controller --> billing-service is redundant: each features: controller --> service (line
  12) already covers it`.
- Isolation. An expansion joins two nodes of one child, so it never joins two children of an
  isolated boundary: a template never trips `%% isolated`.
- Open order. An expansion joins two nodes of one child. The nodes of a child lie in the same layer
  of a chain unless the child itself holds nested layers, so it narrows no pair of layers in the
  ordinary layout, and where it does hold two layers it is a declared edge between them like any
  other (see "Open order").
- Order and no-cycles see the expansions as node edges: an upward expansion is a contract error of
  `%% order`, and a template that closes a cycle is a `%% no-cycles` error. Their messages name the
  expansion `each features: controller --> service` and the `%% each` line.

`brief` and `neighbors` show a pair that only an expansion allows annotated `via each features:
controller --> service, line 12` (the winner of the precedence of "Boundary edges", the same form as
`via features --> core, line 31`); in JSON `via` is `{"edge": "each features: controller --> service",
"line": 12}` and `kind` is the expansion's. Expansions are not listed in the `boundary edges:` block
and not counted in the summary line. The `rules:` block prints the line: `each features: controller
--> service, controller -.-> view`, and JSON `rules.each` is a list of `{"boundary": "features",
"line": 12, "arrows": ["controller --> service", ...]}` in file order, absent when there is none.

`diff` reports a template's change as its directive line added or removed (see "Diff beyond the
contract"), not the expansions it adds or removes. A new feature folder is the diff of its nodes,
which a template does not hide.

A template declares edges, never nodes or ownership, so a new feature does not become covered by
appearing on disk. With the module form of `%% map` (`%% map billing-*: src/features/billing/`) a new
source file in a mapped feature derives a node the contract does not declare, `unknown-node` at the
map line, and a new feature folder that no `%% map` line covers is `unmapped-file` on each of its
source files. Both are errors, so the contract has to name the feature (its boundary and its nodes)
before the template applies to it, and the feature then gets the template's arrows with no more
lines.

#### Budgets

`%% budget <node>: fan-out <n>, files <n>, lines <n>, interface <n>` limits a node's size, reach and
the width of what others use of it. Any non-empty subset of the four measures, in any order, each at most once, each with a whole number from 0; one
line per node, which must be declared. Anything else is a contract error.

- `fan-out`: the number of distinct nodes the node depends on in production code: the observed
  edges (see "Import kind") from the node to another node that have at least one site that is not
  test-only. What code does counts, declared or not: an undeclared pair is also an
  `undeclared-edge`. Type-only, embed and asset sites count, like any dependency. An edge realised
  only by test code does not, nor does an import within the node.
- `files`: the number of source files the node owns, the count `brief` prints as `N files`. Test
  files the node owns count (a node that owns its tests budgets them too); assets and excluded
  files are not source and do not.
- `lines`: the sum of the line counts (newline bytes, plus one when the last line has none) of those
  same source files, the count `brief` already prints for unread files.
- `interface`: the number of distinct symbols defined in the node that code outside the node uses in
  production code, so that a node stays deep: a narrow interface behind a lot of code. An import
  site counts when its importer is owned by another node and it is not test-only; an import within
  the node and a test-only site do not. A site of an undeclared edge counts, as for `fan-out`, and
  so do type-only sites. An entry is identified by the file that defines the symbol and its name.
  The file is the one the import is attributed to, so a re-export is followed to the definition
  (see "Re-exports") and, for a node marked `%% facade`, the facade file is the definer (see
  "Facades"). The same symbol used by ten importers is one entry. Embed and asset sites name no
  code and add nothing.
  - *Imports that name no symbol.* An import that uses no symbol, or uses `*`, must not let a
    caller escape the budget by changing import style. Where the extraction records the members
    reached through the import, those members are the entries. Otherwise the import counts as one
    entry for its target file, shown as `<file> (whole module)`, however many importers or sites
    there are (the entry is per file, not per site). Per language:
    - Rust: a module binding (`use crate::store;`, `as s`, `self` in a group) is covered by the
      items used through its name, which are recorded at their own sites. It counts as a whole-module
      entry only when the importing file uses no named symbol of that same target file. A glob
      (`use crate::store::*;`) is a whole-module entry.
    - TypeScript and JavaScript: members of a namespace import (`import * as s`), of
      `require('x')` bound to a name and of `export *` are not recorded, so each is a whole-module
      entry, and so is a side-effect import (`import './x'`).
    - Python: `import a.b`, `from a import submodule`, `from a import *`,
      `importlib.import_module("a.b")` are whole-module entries (no member access is recorded).
  - A node that nothing outside imports has interface 0.
- Exceeding a limit is the warning `over-budget`, one per measure, at the budget line of the
  contract, with a message naming the node, the measure, the limit and the actual value:
  "node `lint` is over its budget: fan-out 7 exceeds the limit 5". Equal to the limit is within it.
  Its hint suggests narrowing the interface behind fewer entry points for `interface`, or raising
  the limit in the contract, which is the user's call.
  `--strict` fails on it like on any warning. It is a warning because a budget is a guideline that
  a contract may accept while a refactor is planned; the contract is where a limit is raised.
- `brief` and `neighbors` show the budget of a node with its current values, `budget: fan-out 3/5,
  files 21/20 over`, one `actual/limit` per measure, `over` after one that exceeds. In JSON the node
  has `budget` with, per measure that has a limit, `{"limit": n, "actual": n}` (a node without a
  budget line has no `budget` key).
- `neighbors <node>` also lists the interface entries in an `interface:` block, one per entry as
  `store/db.rs open  (3 sites: api 2, cli 1)`, or `<file> (whole module)` for a whole-module entry,
  sorted by site count (most first), then by file and name. A site is one import location of an
  importer outside the node; the users are the nodes of those importers with their site counts. The
  block is shown only when the node has an `interface` limit or `--all-sites` is given, so a node
  without the budget does not get the extra lines by default. In JSON the block is `interface`, a
  list of `{file, symbol, sites, users: {node: n}}` (`symbol` is absent for a whole-module entry),
  present under the same condition.

#### Test isolation

`%% tests: isolated` keeps test code out of production: an import written in production code of a
file that is test code is the error `production-imports-test`, whatever the edges say. `isolated`
is the only value; the line is given at most once.

- Test code is what "Import kind" calls `test_only`, and the rule uses the same classification:
  a file is a test file when its name or place says so for its language (TypeScript, JavaScript,
  Svelte and HTML: `*.test.*`, `*.spec.*`, or under `__tests__`, `test`, `tests`; Python:
  `test_*.py`, `*_test.py`, `conftest.py`, or under `tests` or `test`; Swift: under `Tests`, `Test`,
  `tests` or `test`; shell: `test_*`, `*_test`, or under `test` or `tests`; Rust: a file under
  `tests`, named `tests.rs`, `*_test.rs` or `*_tests.rs`, or a file a `#[cfg(test)] mod` declaration
  pulls in). A file that holds only a `#[cfg(test)]` module is not a test file: the rule is about
  files, and the statements inside the module are test code, so they are never a production site.
- A violation is a resolved import site that is not `test_only` whose target is a test file.
  Test code importing test code, and test code importing production code, are fine, and so is
  production code importing a node's non-test files. The check is per site, not per edge: it
  applies inside one node as well as across nodes, to pairs with a solid edge, a `|test|` edge or no
  edge (an undeclared pair is also `undeclared-edge`), and to files no node owns.
- Only source files are test files. An asset or an excluded file is never one, and a reference
  driftwall skipped (`brief` lists them) is not seen.
- The finding is at the import: "production file `a/x.ts` imports test file `a/helper.test.ts`, but
  `%% tests: isolated` on line 12 keeps test code out of production". One per site. The hint says to
  move the shared code out of the test file or mark the importer as test code. The check is the
  per-helper form of the per-edge `-->|test|` check ("Test edges").
### Package rules

Edges say which nodes may depend on each other. Package rules say which code outside the repository
a node may use: "only `db` imports `sqlx`", "the pure model must not import `lit` or `three`",
"`domain` may not use `node:child_process`". They check the package uses of "Package uses".

```
%% packages db: sqlx, tokio*
%% packages config:
%% forbid-packages domain: node:child_process, subprocess, lit
```

**Syntax.** A comment line whose first word after `%%` is `packages` or `forbid-packages` (any
whitespace around the words, any position in the file, like `exports`). It is
`%% packages <node>: <name>[, <name>...]` or the same with `forbid-packages`. A comment that starts
with one of the two words and does not have this form is a contract error with its line number,
never a plain comment (`%% packages are listed below` is an error). `packages-tests` and other
words are plain comments.

- The node must be a node the contract declares, checked once the whole file is read, so the order
  does not matter: an unknown id is a contract error at the line of the rule, like `exports` and
  `map`.
- A name is the package as "Package uses" names it, written as the code writes it: `lit`,
  `@lit/reactive-element`, `node:child_process`, `subprocess` (the Python import name), `serde_json`
  (a Rust crate with `_`), `Alamofire`. A name has no whitespace and no `,`. It matches the package
  use exactly.
- A name that ends in `*` is a prefix: `@scope/*` matches every package of the scope, `serde*`
  matches `serde`, `serde_json` and `serde_derive`, `node:*` every Node builtin. The `*` is only
  valid last. The same holds for crates and Python and Swift modules: the prefix is applied to the
  name as recorded, and `-` and `_` are not folded, so `serde-*` matches no Rust crate (use `serde_*`).
- Several lines may name one node and one kind: their names add up. The same name twice for one
  node and kind, across lines too, is a contract error at the second line, naming the first. A name
  in both a `packages` and a `forbid-packages` line of one node is a contract error as well (the
  contract would contradict itself); a pattern that merely overlaps another (`packages web: lit*`
  and `forbid-packages web: lit-ssr`) is fine and the next paragraph decides it.
- `forbid-packages` needs at least one name. `packages` may have none: `%% packages config:` is an
  allow-list with no entry, the node uses no third party package at all.

**Meaning.** For each package use written in a file owned by node `n`:

1. A use that matches a name of any `forbid-packages` line of `n` is `forbidden-package`, whatever
   its kind: the deny-list applies to the standard library too (`node:child_process`).
2. Else a `third-party` use is `forbidden-package` when `n` has at least one `packages` line and
   the use matches none of its names. A node with no `packages` line is unrestricted.
3. Else it is allowed.

So a deny-list wins over an allow-list, and an allow-list restricts third party packages only:
`stdlib` uses are not restricted by it. A stdlib name listed in `packages` is accepted and changes
nothing (it only keeps itself from being stale while used). To restrict the standard library, deny
the names. Files with no owner and files owned by an undeclared node produce no finding, as for
edges.

**Tests.** A use written in test code (`test_only`, see "Import kind") is exempt from both kinds
of rule, and there is no rule for tests: a test imports its framework, fixtures and helpers freely.
Test files also do not count as users of an allow-list entry (see staleness).

**Findings.** One `forbidden-package` error per use, at the file and line of the import, naming the
node, the package, its kind and the rule line: "`db` imports `lit` (third-party) but
architecture.mmd:12 `%% packages db` does not list it", or "... but architecture.mmd:13
`%% forbid-packages db` forbids it". Message and hint (remove the import, or ask to approve the
change of the rule) follow "Rules (violation kinds)".

**Staleness.** An entry of a `packages` line that no non-test package use of a file of the node
matches is the warning `stale-package` at the line of the rule, naming the entry and the node, like
`stale-map`. `forbid-packages` entries are never stale: a deny-list that nothing violates is its
purpose. A node that owns no file at all has every entry stale.

`brief`, `neighbors` and `where` show the package uses (see "CLI surface").

## Ownership

A `.arch` file contains `node: <id>` and, optionally, `scope: here` (comments start with `#`; each
directive at most once, `node:` is required). It tags its directory and all subdirectories, until a
nested `.arch` overrides it. Source files are found by walking the repo with the `ignore` crate, so
`.gitignore` is honored. `.driftwallignore` (gitignore syntax) adds exclusions. Only files with a
supported extension count as source. One exception: an HTML file
(`.html`, `.htm`) counts as source only when it holds a `<script` element, see "Svelte and HTML";
any other HTML file is an asset, so documentation and fixture pages never become unmapped files.
A file with no extension counts as source when its first line is a shebang that names a supported
interpreter, see "Source languages by file".

`scope: here` limits the tag to the files directly in its directory: it is not inherited by any
subdirectory, so a source file added below it finds the next `.arch` further up, or none and is
`unmapped-file`. Without `scope:` the tag covers the whole subtree, as above. `here` is the only
accepted value, and a `scope:` line without a `node:` line is the same `missing node` error as an
empty file. The scope is a typed field of the tag (`Scope::Subtree`, the default, or `Scope::Here`).
`where` and `scan` answer through the same rule: a `scope: here` file is skipped for any file that is
not directly in its directory, and the search continues in the directories above it.

### Contract map (`%% map`)

A repository can adopt a hand-designed diagram without any `.arch` file: the mapping from code to
nodes may live in the contract itself, which may sit anywhere (`docs/architecture.mmd`).

```
%% map ui: src/ui/, src/components/**/*.svelte
%% map api: src/api/
%% map ui: src/shared/ui.ts
%% map store-*: src/stores/
```

**Syntax.** A comment line whose first word after `%%` is `map` (any whitespace around the words, any
position in the file, like `exports`). It is `%% map <target>: <glob>[, <glob>...]`. A comment that
starts with the word `map` and does not have this form is a contract error with its line number,
never a plain comment (`%% map of the system` is an error). The target is a node id, which must be a
node the contract declares (declaration order does not matter, as for `exports`), else the error
is reported at the map line. Several map lines may name one node. The line number of every rule is kept
in `Contract::maps` and is not part of the contract's equality, so a contract round-trips through
`render` with its rules.

**Globs** are relative to the repository root (the lint root), whatever directory the contract is in.
A leading `/` or `./` is dropped, so `src/ui/` and `/src/ui/` are the same glob. They are anchored:
`ui/` never matches `src/ui/`. Segments are separated by `/`. In a segment `*` matches any run of
characters and `?` one character, neither crossing a `/`. A segment that is exactly `**` matches any
number of whole segments, none included. A glob that ends in `/` names a directory and matches every
file below it at any depth (`src/ui/` is `src/ui/**`); `./` (or `/`) is the whole repository. Any
other glob matches a file path exactly, so `src/*` is the files directly in `src` and `src/ui`
without a slash matches only a file called `ui`. This is the gitignore dialect `.driftwallignore`
uses, with two differences chosen so that the answer is exact: globs are always anchored, and `*`
never reaches into a subdirectory through a matched directory name. A glob must not be empty or
start with `!`, and has no `..` segment, no `\`, no `[`, `]`, `{` or `}`, and `**` only as a whole
segment. A repeated glob (after dropping `./`) anywhere in the contract is an error at the second
line. The list is split at commas, so a glob has no comma. Globs apply to every walked file, source or
asset or excluded, as a `.arch` tag does, but only a source file can be `unmapped-file`.

**Specificity.** A file belongs to the node of the matching glob that is most specific. The
specificity of a glob is, compared in this order,

1. the number of leading segments that are literal (hold no `*` or `?`): `src/ui/` is 2, `src/*.ts`
   is 1, `**/x.ts` is 0;
2. whether the glob matches files by pattern (it does not end in `/`) rather than as a whole
   subtree: at the same depth `src/*.ts` beats `src/`, and `ui/src/components/*.test.tsx` beats
   `ui/src/components/`. An exact file glob also counts as a file glob;
3. the length of the literal prefix: the literal segments, each followed by the `/`, then the
   literal characters of the first wildcard segment before its first wildcard (`src/ui/a*.ts` is
   `src/ui/a`, 8; `src/ui/` is 7);
4. the length of the literal suffix: for a file glob whose last segment has a wildcard, the literal
   characters after its last wildcard (`*.test.tsx` has `.test.tsx`, 9; `src/*.ts` has `.ts`, 3).

So `src/a*.ts` beats `src/*.ts` (step 3), and `src/*.test.ts` beats `src/*.ts` (step 4).
A deeper directory glob therefore beats its parent, as the nearest `.arch` does. Two globs of the
same node, or of the same module target, never conflict. A tie between the most specific globs of
different targets for one source file is a contract error (exit `2`): `<contract>:<line> and
<contract>:<line> both map <file> with equal specificity (<glob>, <glob>)`, naming the lines and the
first such file in path order. Only source files can raise it: for an asset or excluded file a tie
means no owner, so a tie never fails a scan through a file that is not code. A file no glob matches is
unowned, and `unmapped-file` exactly as without a map.

**Module form.** `%% map <prefix>*: <dir>/` (`%% map *: <dir>/` for no prefix, `<prefix>` as in
`node: <prefix>*`, ending in `-`) makes every module in the directory its own node by the module rule
of "Module nodes" (test files, entry files, folded entry files, leading underscores, a derived id that is
no valid node id is an error, at the map line). It is the whole `<dir>/` subtree with a derived node,
so it takes part in specificity like any directory glob, and a more specific plain glob inside it wins.
The value is exactly one glob, a directory without wildcards ending in `/` (`./` for the root). The
derived nodes are checked against the contract like those of a `node: *` tag: `unknown-node`, located
at the map line.

**One mechanism per repository.** A contract with any `%% map` line decides ownership alone: `.arch`
files are not read, so one with a mistake does not fail the scan. When some exist anyway, `lint`
reports them once as the warning `ignored-tag-files` (count and the first 3 paths, sorted), not a tool
error: a repository can move to maps in steps, and `--strict` makes it a failure when it should be
finished. A contract without map lines works as before.

**Staleness.** A glob that matches no walked file (source, asset or excluded file, ignored files
excluded) is the warning `stale-map` at its line, naming the glob. It is judged on the glob alone, not
on the files it wins, so a glob shadowed by a more specific one is not stale.

**Where.** `where <file>` answers `<node> (via <contract>:<line> (map <glob>))` with the contract path
as given and the glob as written; JSON `arch_file` is the contract path and `map` is `{line, glob}`
(absent for a `.arch` decision). `where` uses maps when the contract (`--contract`, else the default
if it exists and is valid) has map lines. `brief` and `neighbors` show as the `dirs` of a node the
globs of its plain map lines as written, and, for a module-form line, the module paths of the files it
owns, as for a `node: *` tag.

### Source languages by file

The table of file names to languages is data in `src/ownership/language.rs`. Matching is
case-sensitive.

| Language | Files |
| --- | --- |
| TypeScript | `.ts`, `.tsx`, `.mts`, `.cts` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| Python | `.py`, and extensionless files whose shebang names Python |
| Rust | `.rs` |
| Svelte | `.svelte` |
| Html | `.html`, `.htm`, only with a `<script` element |
| Swift | `.swift` |
| Shell | `.sh`, `.bash`, `.zsh`, and extensionless files whose shebang names a shell |

**Extensionless scripts.** Only a file whose name has no extension (`deploy`, `pre-commit`, and a
hidden name such as `.profile`: no `.` followed by text) is sniffed, so `deploy.old` or `run.txt`
never is. The first line must start with `#!` (no space, no byte order mark before it; later lines
are never read, and at most the first 256 bytes of the line). After `#!`, spaces are skipped and the
interpreter is the first word, and the name is the last `/` segment of that word. When the name is
`env`, the interpreter is instead the first following word that does not start with `-` and holds
no `=` (`#!/usr/bin/env -S python3 -u` is `python3`), and again its last segment is the name. Then:

- `sh`, `bash`, `zsh` or `dash` makes the file Shell;
- `python`, or `python` followed only by digits and dots (`python3`, `python3.12`), makes it Python;
- anything else (`node`, `perl`, `fish`, `ksh`, a missing interpreter) does not: the file is an
  asset.

A file whose first line is not UTF-8, or that cannot be read, is an asset, so sniffing never fails a
scan. A sniffed script is source like any other: it is owned or unmapped, excluded under `vendor/`,
and an import target. An extensionless Python script is never imported by Python code: only `.py`
files are Python import targets.

### Excluded files

Minified, vendored and generated code is not the project's own source, so the scan leaves it out by
default. A source file is excluded when

- it is below a directory named `vendor` or `third_party` (at any depth), reason `vendored`; or
- its name has the form `<name>.min.<ext>` (the second to last dot-separated part is exactly
  `min`: `htmx.min.js`, `app.min.mjs`, `site.min.css`), reason `minified`; or
- it is below a directory named `dist` (at any depth), reason `generated`; or
- one of its first 5 lines contains, ignoring case, `@generated`, `do not edit`, `auto-generated` or
  `autogenerated`, reason `generated`. Only the first 5 lines are read, and a line that is not UTF-8
  is read lossily.

A file that matches several gets the first of `vendored`, `minified`, `generated`. Excluded files are
never parsed, never in `Ownership::owners` and never `unmapped-file`: their own imports are not read.
They are not silently dropped: `Ownership::excluded` maps each reason to the sorted set of excluded
paths, so a report can show them. `.arch` files are never excluded.

**Excluded files are import targets.** Leaving a file out of the scan does not make it disappear
from the repository, and an import of it is real code that depends on it. An excluded file is
probed exactly like a source file (see "Probing"), so `./font-metrics.mjs` finds the excluded
`font-metrics.mjs` before it would find `font-metrics.d.mts`, and `./g.mjs` is no
`unresolved-import` when `g.mjs` exists and was excluded. Such an import is a resolved import of kind
`runtime` unless the import says otherwise (see "Import kind"), it records its symbols like any
other, and it is a dependency on the node that owns the excluded file. The owner is decided by the
tag rules of source files, as for assets: `Ownership::excluded_owners` maps each owned excluded file
to its node, an excluded file that no tag covers (or whose derived name under `node: *` is not a node
id) is in no map and is a target with no owner, which makes no edge. When the owner is not a node
the contract declares, the import is recorded as an `undeclared-owner` skipped reference, as for an
asset (see "Asset files"), with the message ``excluded file `<path>` is owned by `<node>`, which the
contract does not declare``. Excluded files stay out of `owners`, `unmapped`, every file count and
every module-node derivation: they only resolve and have an owner.

A project opts files back in with a negated pattern in the `.driftwallignore` at the repository
root (`!vendor/mine/`, `!static/app.min.js`, `!dist/`, `!src/api.gen.ts`): a file whose path that file whitelists is source
again. Only the root `.driftwallignore` is consulted for opt-in; negations in nested
`.driftwallignore` files still work for ignoring (the walker applies them) but do not opt back in.

### Asset files

Every other file the walk reaches (`.json`, `.css`, `.svg`, `package.json`, a README, ..) is an asset.
Assets get an owner by the tag rules of source files: the nearest `.arch` decides, and under `node: *`
the module rule names the node by the file name up to its first `.` (so `a.css` joins `a.ts`).
`Ownership::asset_owners` maps each owned asset to its node and `Ownership::asset_unmapped` lists the
ones no `.arch` covers. Assets are never reported as `unmapped-file` and never create `.arch` tags to
validate, and they change nothing about `owners`, `unmapped`, `excluded` or any file count: a node's
file count is its source files. An asset whose derived module name is not a valid node id
(`.eslintrc.json` under `node: *`) is listed in `asset_unmapped` and is never an error, because an
asset must not be able to fail the scan. The vendored and minified rules (see "Excluded files") apply
to source only, so an asset below `vendor/` is still an asset. `.arch` files are not assets, and ignored
files (`.gitignore`, `.driftwallignore`) are not walked, so they are neither assets nor resolvable.

An import of an asset whose owner is not a node the contract declares (a tag names an undeclared
node, or `node: *` derives one the contract lacks, so no edge can exist) is not silent. When the
importing file is owned by a declared node, `lint` records it as a skipped reference with reason
`undeclared-owner` (see "Skipped references"). Its specifier is the repository path of the asset,
since the import as written is not kept, and its message is ``asset `<path>` is owned by `<node>`,
which the contract does not declare``. It is attributed to the node of the importing file, shows in
`brief` and `neighbors`, and is no lint finding. An import of a source file keeps its behaviour:
a target owned by an undeclared node is no edge and no record, as `unknown-node` already reports
the tag. An asset that no tag covers, and an importer that no declared node owns, produce no record.

### Blind spots

What driftwall cannot see must never be silent. Beside the source files it reads, the scan keeps a
census of every file it leaves unread, and the import collection keeps every reference it cannot
follow. The census is data, never a finding: it changes no owner, no `unmapped-file`, no edge and no
exit code. It lives on `Ownership` next to `excluded`:

- **Not read**, `Ownership::unread`: a map from a key to the sorted set of walked files of that key.
  The key is the file extension (no dot, case-sensitive) when it is in the fixed table of common code
  extensions `sh bash zsh swift c h cc cpp hpp m mm go java kt kts rb php cs lua dart scala ex exs
  erl hs ml clj vue astro pl r sql ps1 cmd bat mc zig nim svelte html htm`, minus every extension the language
  table supports (so adding a language removes its extensions from `unread` by itself: `ts` is never
  here). A file with no extension at all whose first two bytes are `#!` has the key `shebang`. The
  files are the walked ones: ignored files (`.gitignore`, `.driftwallignore`) are not walked and are
  not counted here. An unread file is also an asset and resolves as one, as before.
- **Ignored**, `Ownership::ignored`: the files with a source extension that only `.driftwallignore`
  hides. The scan finds them by walking a second time with the `.driftwallignore` files disabled
  (`.gitignore` stays on: a file git ignores is not part of the repository, so it is never counted),
  and keeping the files of the second walk that the first did not see. The second walk only happens
  when the first one met a `.driftwallignore`. A hidden file in a language without support is not
  counted: nothing would have read it anyway.
- **Excluded**, `Ownership::excluded`, as described above.
- **Skipped references** and **unresolved imports**, the `skipped` and `unresolved` lists of
  `FileImports` (see "Skipped references").

Every census file is attributed to the node that owns its path by the tag rules of source files (the
nearest `.arch`, `scope: here` and `node: *` included, see `owner_in`), or to no node when no tag
covers it, which reports call `unmapped`. A reference is attributed to the node that owns the file it
is written in. Unread, excluded and ignored files never produce `unmapped-file`.

`lint::observe` builds the whole picture as one `BlindSpots` value (`Observation::blind_spots`): each
unread file with its owner and its number of lines (newline bytes, plus one for a last line without a
newline; a file that cannot be read has 0), each excluded and ignored file with its owner, each
skipped reference with its reason and each unresolved import with its owner. All of it is sorted, and
`init` builds the same value without owners and lines, and without the `undeclared-owner` records
only `lint` can make (they need a contract). The two notes therefore agree on a fresh baseline, because
`init` declares every owner it gives an asset (see "Contract and writing"), and differ only by the
`undeclared-owner` references a hand-edited contract adds.

Reports: see "CLI surface". The note line that `lint` and `init` print has the form

```
note: 50 files not read (swift 40, sh 10), 3 skipped references; see driftwall brief
```

Its parts, each present only when non-zero and in this order, are `N files not read (ext n, ..)`
(extensions by file count, descending, then by name, at most 6 then `+N more`), `N files excluded
(reason n, ..)`, `N files ignored by .driftwallignore`, `N skipped references` and `N unresolved
imports`, joined by `, `, then `; see driftwall brief`. With nothing to say there is no note line.

### Module nodes (`node: *`)

A `.arch` file may contain `node: *` instead of an id. Then every module directly in that
directory is its own node, named by a fixed rule, with no further tagging:

- A source file is a module. Its node id is the file name up to the first `.`, so `gitlab.rs` is
  `gitlab` and `gitlab.test.ts` joins `gitlab`.
- **A test file never creates a node of its own.** A file is a test file when its name has a part
  `test` or `spec` between two dots (`a.test.ts`, `a.b.spec.d.ts`, `snap.test.json`), or is
  `test_<x>.py`, `<x>_test.py`, `<x>_test.rs` or `<x>_tests.rs` (`<x>` not empty). Its name is
  derived as above (`rows.test.ts` is `rows`, `test_rows.py` is `test_rows`), but it keeps that
  name only when a non-test module directly in the same directory has it: a source file that is
  not a test file and derives it, or a subdirectory of that name that holds source files at any
  depth. Otherwise it joins the node of the directory itself, the node an `index.*` file there
  gets (`rows.test.ts` next to no `rows.*` module, in `src/`, is the node `src`; in the repository
  root it is `root`). So `rows.test.ts` beside `rows.ts` is `rows`, and every orphan test of a
  directory is one and the same node. The rule applies to assets too, and to files directly in
  the directory only: a file in a subdirectory is named by the subdirectory, as above. The
  non-test modules of a directory are fixed by the files the scan sees, so `lint`, `where` and
  `init` agree.
- Entry files (`mod.rs`, `lib.rs`, `main.rs`, `index.*`, `__init__.py`, `__main__.py`) are named by
  their file stem too (`main`, `lib`), except `mod.rs`, `index.*`, `__init__.py` and `__main__.py`,
  which are named after their directory: `__main__.py` joins its directory's node like
  `__init__.py` does.
- A subdirectory whose name equals a sibling module's id (Rust `gitlab.rs` plus `gitlab/`) belongs to
  that module's node. Any other subdirectory is one node named after the directory.
- Leading underscores are stripped from every derived name, file or directory: `_private.py` is
  `private`, `__tests__/` is `tests__`, `_x.py` and `x.py` share the node `x`. A name that is empty
  after stripping (`_.py`, `__/`, a hidden file such as `.eslintrc.js`) is invalid.
- **A file that declares nothing never creates a node of its own (a folded entry file).** An entry
  file is one named `__init__.py`, `lib.rs` or `main.rs`. It *declares nothing* when its text holds
  nothing but comments and

  - Python: string-literal statements (a docstring); any other statement, `pass` included, is a
    declaration;
  - Rust: attributes and `mod` or `pub mod` (also `pub(..) mod`) declarations without a body. A `use`
    or `pub use` is a declaration (a re-export depends on what it re-exports), and so is any item
    or an inline `mod x { }`.

  The check is textual and any doubt means "declares something". Such a file is *folded* when no
  other module of the directory has the name it would derive (`mybot` for `src/mybot/__init__.py`,
  `lib` for `src/lib.rs`): the modules that decide this, the `peers`, are those of the files that do
  not declare nothing, so an entry file never keeps itself alive. A folded file stays in the region
  but is in no owner map: neither owned nor `unmapped-file`, no node derived from it, and it is
  listed in `Ownership::folded`. It is still a known file, so an import of it resolves, and since
  nothing owns it the import makes no edge and no record: it has nothing to depend on. When a module
  has the name (`lib.rs` beside `lib/x.rs`, `__init__.py` beside a module named like its directory),
  the file joins it as before. The rule applies to files in subdirectories too: a subdirectory
  holding only a folded file derives no node. It applies under `node: *` only: an entry file under a
  plain `node: <id>` tag belongs to that node. Because reading the file is needed, ownership reads
  the entry files of the scan (`lint`, `init`, `where`, `explain`), a handful per repository; `lint`,
  `where` and `init` use the one rule (`ownership::folds`) and agree.
- A nested `.arch` still overrides, as everywhere else.
- A derived id that is not a valid node id is an error at the `.arch` file, never silently renamed.

`node: <prefix>*` derives the same modules but gives each id the prefix: the node of `components/`
under `node: ui-*` is `ui-components`. A prefix matches `[A-Za-z][A-Za-z0-9_-]*-` (it ends in `-`);
anything else before the `*` is an invalid id error at the `.arch` file. The prefix is a typed part
of the tag (`Tag::Modules(Prefix)`; a plain `*` has the empty prefix). The derived id is the prefix
followed by the module name, and it must be a valid node id with a non-empty module name, so `ui-`
plus `9lives` is valid but `ui-` plus the empty name is not.

`init` writes `node: *` into a directory that directly holds two or more modules (see `init`). An
entry file directly in the repository root (`index.ts`, `__init__.py`) is named `root`, since the
root directory has no name of its own. `unknown-node` reports a derived node the contract does not
declare at the `node: *` line of its `.arch` file.

## Import semantics

- An import is an edge only when it resolves to a repository file owned by a different node. The
  file is a source file, an excluded file (see "Excluded files") or, for TypeScript and JavaScript,
  an asset (see "Asset files").
- Imports inside one node are ignored. Third party and standard library imports are not edges: they
  are recorded as package uses, see "Package uses".
- A relative import that resolves to nothing is an `unresolved-import` warning, never silently
  dropped, whatever extension the specifier has (`./missing.json` included). So is a Python absolute
  import into a first-party package that holds no such module, see "Python resolution".
- Files with no owner never produce edge findings (no cascade from `unmapped-file`).
- Resolution per language: TypeScript and JavaScript are described under "TypeScript and JavaScript
  resolution" below, Svelte and HTML under "Svelte and HTML" (their scripts resolve exactly like
  TypeScript and JavaScript). Python handles relative imports, `__init__.py`, and absolute imports against
  an ordered list of import roots, see "Python resolution" below. Rust handles `use crate::`, `use super::`, and `mod` declarations are
  containment, not edges; see "Rust resolution" below.
  A named import through a module that re-exports the name is attributed to the file that defines
  it, see "Re-exports".
  Swift imports resolve through the targets of `Package.swift` (see "Swift resolution") and shell
  `source` commands through literal and script-relative paths (see "Shell resolution").
- Extraction is tree-sitter based and driven by a per-language table (extensions, grammar, query),
  not a trait hierarchy. Adding a language means adding a table entry and a query.

### TypeScript and JavaScript resolution

A specifier is resolved in this order, the first step that applies deciding.

1. **Suffix.** A bundler query or fragment is not part of the file name: everything from the first
   `?`, or from the first `#` that is not the first character, is removed before resolution
   (`./a.svg?raw`, `./w.ts?worker`, `./a.css?inline`, `./x.png?url&v=2`, `./a#frag`); a `raw`, `url`
   or `inline` query decides the kind of the import, see "Import kind". Messages
   (`unresolved-import`, skipped references) keep the specifier as written. A specifier that starts
   with `#` is a package `imports` key.
2. **Relative** (`.`, `..`, `./x`, `../x`): the path is joined onto the directory of the importer and
   probed, see "Probing". A relative specifier that probing does not satisfy is an
   `unresolved-import`, whatever its extension.
3. **Aliases.** The `paths` and `baseUrl` of the governing config (see "Governing config") yield
   candidate paths that are probed like a relative path. Then the `resolve.alias` entries of the Vite
   config that governs the file (see "Vite aliases") are tried.
4. **Workspace packages**, see below.
5. **Package `imports`**, for a specifier that starts with `#`, see below.
6. **Package check.** A Node builtin or a declared dependency is not a repository import: it is a
   package use (see "Package uses"). Anything else is skipped as `undeclared-package`, and is a
   package use too.

The first of steps 3 to 5 that finds a file wins. A `paths` pattern that matches the specifier and
finds no file is an alias: it ends the resolution silently unless a Vite alias, a workspace package
or an `imports` key resolves the specifier.

#### Probing

A candidate path (a relative path, an alias target, a package entry) is looked up against the files
the scan knows. The first that exists wins, in this order:

1. the path itself as a source file (an excluded file counts as one, see "Excluded files"), then the
   path itself as an asset (see "Asset files"). A candidate
   written with a trailing `/` is a directory and skips this step and the next two;
2. the path with an extension appended, in this order: `ts`, `tsx`, `d.ts`, `mts`, `cts`, `d.mts`,
   `d.cts`, `js`, `jsx`, `mjs`, `cjs`, `svelte`. The extension is appended, so `user.service` finds
   `user.service.ts`. `svelte` comes last, so a specifier that resolves without it never changes
   meaning when a component of that name is added. Neither TypeScript nor Vite appends `.svelte`
   themselves (the Svelte tooling expects the extension written out), so this step is a leniency of
   driftwall and not a mirrored behaviour: `import X from './X'` finds `X.svelte` only when no
   TypeScript or JavaScript file of that name exists;
3. when the path is written with a JavaScript extension, the TypeScript files that compile to it:
   `.js` finds `.ts`, `.tsx`, `.d.ts`; `.jsx` finds `.tsx`; `.mjs` finds `.mts`, `.d.mts`; `.cjs` finds
   `.cts`, `.d.cts`;
4. `index.<ext>` in that directory, with the extensions of step 2.

Step 1 is why an existing file always wins over an appended extension: `../b/both.svelte` is the
component `both.svelte` even when `both.svelte.ts` (a Svelte 5 module) exists next to it, and
`../b/both` finds `both.ts` or another listed extension before `both.svelte`.

A declaration file (`decl.d.ts`) is a source file like any other, so
`import type { D } from '../types/decl'` resolves to it. A probe never adds an asset extension: `./data`
does not find `data.json`.

An import whose probe ends on an asset is a resolved import of kind `asset` that uses no symbol. This
holds for relative specifiers, alias targets and package entries alike.

#### Governing config

A file is governed by the `tsconfig.json` (else `jsconfig.json`) in its own directory or, failing that,
in the nearest ancestor directory. A solution-style config (`"files": []` and non-empty
`references`) that defines neither `paths` nor `baseUrl` after `extends` has no aliases of its own.

When the nearest config is solution-style, its `references` are candidates, in the order written. Each
`path` is a directory (its `tsconfig.json`, else `jsconfig.json`) or a config file, relative to the
solution config. A reference that is missing, malformed, or leaves the repository is skipped, and a
referenced config that is itself solution-style is not followed further. The file is governed by the
first referenced config that includes it, and that config's `paths` and `baseUrl` (after its own
`extends`) apply. When no reference includes the file, the solution config is as if it were absent: the
nearest config above it applies, as it did before references were read.

A referenced config includes a file when

- the file is listed in its `files` (a path relative to the config), or
- the file matches a pattern of `include` and no pattern of `exclude`. A config with neither `files`
  nor `include` has the `include` `**/*`, every file below its directory.

`include`, `exclude` and `files` are read from the config or, when it does not set one, from the
nearest config of its `extends` chain that does, relative to the directory of the file that sets it.
Patterns are relative to that directory: `*` matches any characters inside one path segment, `?` one
character, `**` any number of directories (also none). A pattern whose last segment has no `.`, `*`
or `?` names a directory and everything below it, so `src` includes `src/a/b.ts`; any other pattern
names files, so `src/*` includes `src/b.ts` and not `src/a/b.ts`. A leading `./` is ignored and a
pattern that leaves its directory matches nothing. `exclude` does not apply to `files`. `compilerOptions` such as `rootDir` and the file
extensions TypeScript would compile do not decide inclusion.

#### Vite aliases

A Vite config (`vite.config.js`, `.mjs`, `.ts`, `.cjs`, `.mts` or `.cts`, the first in that order when a
directory holds several, and only a config that is a known file) governs the files at or below its
directory. Its `resolve.alias` is read like the `root` option (see "Vite root"): the config is never run,
and the object is found by the same rules, only a `resolve` key directly in it counts and an
`alias` key directly in that. The alias is read when it is

- an object, `{ 'mrc-wasm': <path>, other: <path> }`, each key a plain string literal or a plain
  identifier, in the order written;
- an array of objects, `[{ find: 'x', replacement: <path> }]`, in the order written, `find` a plain
  string literal and `replacement` a path.

An entry whose key or `find` is anything else (a regular expression, a computed key, a spread) or
whose path is not recognised is ignored. A `<path>` is read with the forms the Vite root reader
understands: a plain string literal, relative to the config directory (`'./src'`), `__dirname` and
the other forms of the config directory, `path.resolve(__dirname, 'a', 'b')` and `path.join(..)`,
`fileURLToPath(new URL('./a', import.meta.url))`, also written `new URL(..).pathname`. A path that is
absolute or leaves the repository is not recognised.

A specifier matches an entry when it is the key or starts with the key and `/`. The first entry that
matches, nearest config first (a config in a directory nearer to the file than another one is tried
before it), decides; the rest of the specifier after the key is appended to the path. That target is
probed like a relative path (see "Probing"), so an alias to a directory finds its `index` file, and
the import resolves like any other: a source file, an excluded file or an asset. When it finds no
file:

- the alias target exists in the repository when some known file (source, asset or excluded) is at
  or below it. Then the specifier is an alias with no file behind it and ends the resolution
  silently, as a `paths` pattern does;
- otherwise the target does not exist (typically a build output that git ignores: `engine/wasm/pkg`
  for `'mrc-wasm'`) and the import is skipped as `alias-target-missing`, never as `undeclared-package`
  and never silently: its message names the alias and the target, ``alias `mrc-wasm` points to
  `engine/wasm/pkg`, which has no file in the repository``. In a report that knows the owners
  (`lint`, `brief`, `neighbors`) the message ends with ``; the target lies in the directory of node
  `engine-wasm` `` when a tag covers the directory the target is in (the nearest `.arch` above it).

When the owner of the missing target is known (`lint`, which has the ownership), the import is also
a dependency on that node, an alias to a build output: the directory holding the target is owned by
a node through a `.arch` tag or a `%% map` glob (the target path is judged like a file directly in
that directory tree, so a subtree glob covers it). The import then counts as an import site of the
edge from the importing file's node to that node, of kind `runtime`, not test code unless the file
is test code, using no symbol (the build output's exports are unknown, so `private-symbol` never
fires on it). A solid edge is realised by it, a planned edge is `planned-realized`, and with no
declared edge it is `undeclared-edge`, so a build output is as checked as a source file. The site
carries `via: "alias-to-build-output"` (JSON `sites[].via`, absent for a plain import) and `brief`
and `neighbors` count it as the property `alias-to-build-output` (see "Import kind"). A target in a directory no node
owns, or inside the importing file's own node, makes no edge. The import stays listed among the skipped
references as `alias-target-missing`, so the blind spot stays visible: the edge exists, but the
symbols and the file are not checked.

The Vite aliases apply after `paths`, so a `tsconfig` alias that finds a file wins, and before the
workspace packages, the package `imports` and the package check.

#### Workspace packages

A `package.json` with `workspaces` (an array of patterns, or an object whose `packages` is such an
array) declares workspace members. Every such `package.json` counts, at the repository root or nested.
A pattern is relative to the directory of its `package.json`, uses `*`, `?` and `**` as above, and a
pattern that starts with `!` removes directories the earlier patterns matched. A pattern is matched
against the directories that hold, or lie above, a TypeScript or JavaScript source file, so ignored
trees are never consulted. A matched directory is a member when its `package.json` is readable and has
a non-empty string `name`. When two members share a name the first in path order wins. pnpm and lerna
workspace files are not read.

A bare specifier whose package name (`@x/b` for `@x/b/util`, `pkg` for `pkg/sub`) is a member
resolves into that member, before the package check and whether or not the importer lists it as a
dependency. Resolution takes the subpath of the specifier, `.` for none and `./util` for `@x/b/util`:

1. When the member has `exports`, the subpath is looked up in it. `exports` is a subpath map when it
   is an object with a key that starts with `.` (keys that do not are ignored); any other value (a
   string, an array, an object of conditions) is the target of `.`. An exact key wins; otherwise among
   the keys with one `*` that match, the longest prefix wins, then the longest key. The `*` stands for
   the matched text, replaced at every `*` of the target. A target is a string starting with `./`, a
   conditions object, or an array of targets tried in order; `null` and any other value yield nothing.
   A conditions object is searched in the fixed order `types`, `import`, `default`, `require` (never
   in the order the keys are written), other conditions are ignored, and the search descends into
   nested objects with the same order. Every target the search yields, in that order, is probed
   against the member directory and the first that names a file wins. So a `types` target that is not
   in the repository (a build output) falls through to the next condition.
2. When the subpath is `.` and `exports` found no file, the fields `module`, `main` and `types` are
   probed in that order against the member directory, then the directory itself (its `index.*`).
3. When the subpath is longer and `exports` found no file, the rest of the specifier (`util`) is
   probed against the member directory like a relative path.
4. Otherwise the specifier is ignored: the package is a repository member whose entry is not a known
   file (a build output), which is not an error.

#### Package `imports`

A specifier that starts with `#` is looked up in the `imports` field of the nearest `package.json` at
or above the importer that has an `imports` object. Its keys start with `#`; the matching rules, the
`*` patterns, conditions and the probing of targets are those of `exports` above, and targets are
relative to the directory of that `package.json`. A target that does not start with `./` names a
package, not a repository file, and yields nothing. A key that matches and finds no file ignores the
specifier. A specifier that no key matches, or one with no `imports` field to look in, goes on to the
package check.

#### Packages

A declared dependency is a name listed in `dependencies`, `devDependencies`, `peerDependencies` or
`optionalDependencies` of any `package.json` at or above the file, up to the repository root, because
Node searches every ancestor `node_modules`. A name declared in any one of them is declared.

#### JSDoc type imports

A block comment that starts with `/**` in a TypeScript, JavaScript or Svelte script may name a module
in a type: `@type {import('../../src/types').ImageManifest}`, `@returns {import('./x').T}`,
`@param {import("y")} p`. Each such `import(` is an import of kind `type-only`, resolved like the
specifier of an `import` statement (relative, aliases, workspace packages, package check, assets,
queries), so an unresolvable relative one is an `unresolved-import` and an undeclared package is
skipped as `undeclared-package`. It is read by this form only: the word `import` not preceded by an
identifier character or a `.`, optional whitespace, `(`, optional whitespace, a string in single or
double quotes with no escape, optional whitespace and `)`. When `.` and an identifier
follow the `)`, the import uses that member as its symbol (`ImageManifest`, `T`); with none it uses
no symbol. Its location is the line of the `import` word, inside the comment. A comment that starts
with `/*` but not `/**`, a `//` comment, a string and a `/**/` comment are not read. The comment
is found by the grammar, so a `/**` inside a string literal is not one. It is test code by the file
rules of "Import kind". A JSDoc `@import {T} from './x'` tag is not read.

#### File references (`new URL`)

`new URL('<path>', import.meta.url)` in TypeScript, JavaScript, Svelte and HTML scripts names a file
relative to the file it is written in: Vite bundles a source file reached this way as a module or
worker and copies any other file as an asset, and tests use it to spawn scripts and read fixtures. It
is read by this form only (no heuristic for any other string that looks like a path): a `new`
expression of the plain identifier `URL` whose first argument is anything and whose second argument is
exactly `import.meta.url`. Its location is the line the `new` expression starts on.

- A first argument that is not a plain string literal or a template without substitutions is skipped
  as `dynamic-specifier`.
- The literal is joined onto the directory of the importing file, `.` and `..` folded, with no
  probing (a file reference names the exact file, so no extension is appended and no `index` file is
  tried) and with a `?` or `#` suffix removed. A literal with a URL scheme (`https:`, `data:`) is not
  a repository file and is ignored.
- A known source file (excluded files included) is a resolved import of kind `runtime` that uses no
  symbol, so it is never `private-symbol`, and still a dependency on that file's node.
- An existing non-source file is an `asset` import, as an import of it would be.
- A target that is a directory, does not exist, or leaves the repository (a leading `/` included) is
  skipped as `file-reference-missing`, with a message naming the repository path looked for (`.` for the
  root) and saying `is a directory`, `has no file in the repository` or `leaves the repository`. It is
  not an `unresolved-import` and so never a finding: unlike an `import`, the file may be created at
  run time or by a build, and the skip keeps it visible in `brief`.

### Python resolution

A module reference is the dotted module of `import a.b`, `import a.b as c`, `from a.b import c`,
`from .a import c` and the literal dynamic imports below. Spaces inside it are ignored.

**Relative** (leading dots): the module is joined onto the directory of the importer, climbed one
level for each dot after the first. A relative import that climbs above the repository root or finds
no file is an `unresolved-import`.

**Absolute** (no leading dot): the module is looked up under each import root of the importing file,
in order, and the first root that holds a file wins.

#### Import roots

The import roots of a file, in this order, with a later duplicate of an earlier root dropped:

1. **Own directory**, when the directory of the importing file holds no `__init__.py`. Python runs a
   script, and pytest in its default import mode runs a test module, with that directory first on
   `sys.path`, so sibling files import each other by bare name. A file in a package directory (one with
   an `__init__.py`) never gets this root: its siblings are reached through the package, or relatively.
2. **Configured roots**, see below.
3. **The repository root.**
4. **`src/`** at the repository root.

#### Configured roots

`collect` reads project configuration once. A configuration directory is the repository root and every
directory below it that holds a `pyproject.toml`, among the directories at or above a Python file. In
each, these files are read, in this order, and give roots in the order written:

| File | Setting | Root |
| --- | --- | --- |
| `pyproject.toml` | `[tool.pytest.ini_options] pythonpath` | each entry |
| `pyproject.toml` | `[tool.pyright] extraPaths`, then `[tool.basedpyright] extraPaths` | each entry |
| `pyproject.toml` | `[tool.setuptools] package-dir`, the entry whose key is `""` | its value |
| `pyproject.toml` | `[tool.setuptools.packages.find] where` | each entry |
| `pyproject.toml` | `[tool.hatch.build.targets.wheel] packages` | the parent directory of each entry (`src/x` gives `src`, `x` gives `.`) |
| `pyproject.toml` | `[tool.poetry] packages`, each table with an `include` | its `from` (`.` without one) |
| `setup.cfg` | `[options] package_dir`, the entry with an empty key (`= src`) | its value |
| `setup.cfg` | `[options.packages.find] where` | each entry |
| `setup.cfg` | `[tool:pytest] pythonpath` | each entry |
| `pytest.ini` | `[pytest] pythonpath` | each entry |
| `pyrightconfig.json` | `extraPaths` | each entry |

A root is relative to the directory of the configuration that names it, `.` being that directory
itself; it is normalised lexically. An entry that is absolute, leaves the repository, or is not a
string is dropped. A list setting is an array of strings in TOML and JSON, and in the INI files a list
separated by commas or whitespace (a lone string works everywhere). A `package-dir` entry for a named
package, other tables of `pyproject.toml` and every other file (`tox.ini`, a `Makefile`, an
environment variable such as `PYTHONPATH`) are not read. A file that cannot be read or parsed, or a
setting of the wrong type, is skipped silently like a malformed `Cargo.toml`, and the other files still
count. `pyrightconfig.json` is JSONC.

The roots a configuration directory gives apply to the files at or below it. For a file, the roots of
the nearest configuration directory come first, then those of each directory above it up to the
repository root. So a root-level `pythonpath = ["scripts", "."]` applies to the whole repository, and
a `lib` configured in `services/api/pyproject.toml` means `services/api/lib` for the files below
`services/api` only.

#### Probing

A module `m` with imported names `n` (none for `import m`, and for `from m import *`) resolves under
one root to the first of these files that exists, `<root>/` being omitted for the repository root:

1. for each name, in the order written, and independently: the submodule `<m>/<n>.py`, then the
   package `<m>/<n>/__init__.py`;
2. the module `<m>.py`, then the package `<m>/__init__.py`. With names this is the file the names
   are found in. `from . import x` has the package `__init__.py` as its module.

The first root with a hit decides, so a name that is a submodule under one root and a plain name
inside another module under an earlier root resolves under the earlier. An import of the importing
file itself is dropped.

#### Misses

An absolute module that no root resolves is decided by its first segment, the top-level name:

1. when some root holds a first-party module or regular package of that name (`<root>/<name>.py`, or
   a directory `<root>/<name>/` that holds an `__init__.py`) the import is an `unresolved-import`, as
   a relative miss is. Exception: the module itself exists as such a directory and the miss is only a
   name imported from it, which is ignored;
1a. else when some root holds a directory `<root>/<name>/` with no `__init__.py` (a namespace
   directory, below which any Python file lies) the import is skipped as `undeclared-package`, with a
   message naming the directory. A namespace directory is first-party only for the paths that exist
   below it (those resolve as usual): in Python an installed regular package of the same name beats
   a namespace portion, so a miss below it is most likely that package, as with any undeclared
   third party. The existing reason is reused so the reference stays visible, and the import is not
   a finding. Exception as in 1: the module itself exists as the directory;
2. else when some repository path matches the name, anywhere, the import is skipped as
   `unreachable-module`. Only a Python module or package matches: a file `<name>.py`, or a
   directory `<name>/` that holds an `__init__.py` or another `.py` file directly. A directory that
   merely has Python files somewhere deeper (a Rust crate `engine/dizione` with a script in
   `engine/dizione/scripts/`) is not a Python package and never matches. Its message names the
   first such path in path order and says that no import
   root of the importing file reaches it. This is how a script reachable only through a `Makefile`
   `PYTHONPATH` is seen. When the top-level name matches nothing (`controllers/` holds only a
   subdirectory), the dotted module is matched as a whole instead: the longest prefix of it, at
   least two segments, that is a path suffix of a module file `<p>.py` or a package
   `<p>/__init__.py` anywhere in the repository, or the whole module when it is a path suffix of
   a directory that holds Python directly (`from c.g.m import X` matches `a/c/g/m.py`). The first
   such path in path order is named, with the matched dotted prefix, and the message names only
   Python paths;
3. else it is a third party or standard library module: not an import, but a package use (see
   "Package uses").

#### Dynamic imports

`importlib.import_module(x)` and `__import__(x)` (the call spelled exactly so; a bare
`import_module(x)` is not recognised) resolve like `import x` when `x` is a plain string literal:
no prefix but `r` or `u`, no escapes, no f-string, no implicit concatenation. The import has kind
`runtime`, is located at the start of the call and uses no symbol (a module import, see "Symbols").
Its specifier in `unresolved` and `skipped` is the string contents.

A literal that starts with dots is relative to a package. The package is the second argument, given
positionally or as `package=`, and must be a plain string literal, an absolute dotted name. The module
is that name with `dots - 1` trailing segments removed, then the rest of the literal appended, and it
resolves as an absolute import (roots, probing, misses). When the package is missing, not a literal,
or has too few segments, the call stays `dynamic-specifier`, with the string contents as specifier.
Every other argument (a name, a call, an f-string, a concatenation, a bytes or escaped string, a
literal that is not a dotted name) stays `dynamic-specifier` with the source text of the argument as
specifier.

#### File loaders

`importlib.machinery.SourceFileLoader(name, path)` and `importlib.util.spec_from_file_location(name,
path)` (spelled exactly so) load a Python file by path. When `path`, the second argument, is a plain
string literal (as above) that holds no `*`, `?`, `[`, `{`, `%` or `$`, does not start with `/` or `~`,
and names a known source file, either relative to the importing file's directory (tried first) or to
the repository root, the call is an import of that file, kind `runtime`, located at the start of the
call, using no symbol. The path is normalised lexically and may not leave the repository. Every other
path (a name, a call such as `os.path.join(..)`, a glob or format string, an absolute path, a file
that is not known) is skipped as `dynamic-specifier`, with the source text of the argument as
specifier; a literal that finds no file is not `unresolved-import`, because the file may be made at
run time or live outside the repository.

### Rust resolution

A Rust path is a dependency when it starts with `crate`, `self`, `super` or the name of a library
crate in the repository, and the file that holds the item it names is another repository file. The
file is the longest prefix of the path that names a module file; `mod foo;` is containment and is
not an edge. A path that finds no module file falls back to the crate root file.

- **Names in scope.** A path may start with a name the same file brought into scope with `use`
  (`use crate::gitlab;`, `use crate::gitlab as gl;`, `self` in a group) or `mod name;`. It resolves
  through that binding only when the binding stands for a repository module file. A name bound
  twice to different paths in one module is ambiguous and is not followed, and a binding is never
  followed through another binding. A `use` inside a function binds for the whole enclosing module.
- **Scope is per module block.** A binding is visible in the module block that declares it and in
  no other: an inline `mod tests { }` does not inherit the names of the file around it.
- **Glob from the parent.** An inline `mod` that contains `use super::*;` sees every name its
  parent module block binds, including names the parent got by a glob of its own, `self` module
  aliases and renamed (`as`) items. `use super::name;` and `use super::{a, b}` bind `name` to
  what the parent binds. A name the child binds itself wins over the glob. A call site reached
  this way is recorded like any other, so a call in `#[cfg(test)] mod tests` is `test_only`.
  Limits: only a glob of exactly one `super` is followed (`use super::super::*;` is not), and a
  module in a separate file (`mod tests;` with `use super::*;` in `tests.rs`) does not inherit the
  `use` declarations of the parent file, since a file is read on its own.
- **`#[path = ".."]`.** `#[path = "b/impl.rs"] mod a;` declares module `a` at that file. The path
  is relative to the declaring file's directory; inside inline modules it is relative to the
  directory of a mod-rs file (`lib.rs`, `main.rs`, `mod.rs`) extended with the inline module names,
  and for any other file to its directory extended with the file stem and the inline names. Paths
  `crate::a::..`, `use crate::a;` and `mod a;`-relative paths resolve to that file, and the file's
  own `self::` and `super::` paths are relative to module `a`. Modules the file itself declares
  with a plain `mod x;` are found in its directory, but their own `self::` and `super::` paths keep
  the module path their file location suggests. A `#[path]` that names no known repository file
  is not followed, so a path through it falls back to the crate root file. A `#[path]` inside
  `#[cfg_attr(..)]` and a `#[path]` written in a file that a `#[path]` itself placed are not read.
- **Crates outside the repository.** A path that starts with `std`, `core`, `alloc` or a dependency
  of the nearest `Cargo.toml` is a package use, not an edge, see "Package uses".
- **Not followed:** `extern crate` renames, names that only a glob of a non-module brings in
  (`use crate::*; gitlab::x()`), local names from an inline `mod name { }`, and macro-generated
  paths.

### Swift resolution

Files of one Swift module never import each other: the compiler sees the module as a whole. The
dependencies are between modules, so Swift imports resolve to modules that a `Package.swift`
declares, never to files by path.

**Targets.** `collect` reads every known file named exactly `Package.swift` and finds the calls
`.target(`, `.executableTarget(`, `.testTarget(`, `.macro(` and `.plugin(` (the leading `.` written,
in any position of the file). The manifest is parsed with the Swift grammar, so comments and strings
never produce a target. For each call:

- `name:` and `path:` are read from the arguments of the call itself, never from a nested call
  (`dependencies: [.product(name: "X", package: "Y")]` names nothing). Each must be a plain string
  literal: no interpolation, no escape. A target whose `name:` is missing or not such a literal, or
  whose `path:` is present and not such a literal, is skipped, because its directory is unknown;
- a call nested inside another matched call (`dependencies: [.target(name: "Core")]`) is a reference
  to a target and declares none;
- `.plugin(` declares a target only when it has a `capability:` argument; without one it is the
  plugin product of `products:`;
- the directory of the target is `path:` or, without it, `Sources/<name>` (`Tests/<name>` for
  `.testTarget(`), joined onto the directory of the `Package.swift` and normalised lexically. A
  directory that leaves the repository is dropped. `sources:` and `exclude:` are not read, and the
  fallback directories SwiftPM tries (`Source`, `src`, `srcs`) are not consulted;
- the module name is the target name with every character other than ASCII letters, digits and `_`
  replaced by `_` (`app-cli` is `app_cli`), as SwiftPM names it.

A manifest that cannot be read declares no targets, like an unreadable `Cargo.toml`.

**Imports.** `import X`, `@testable import X`, `@_exported import X`, `import X.Y` and
`import struct X.Y` are read from the `import` declarations of the grammar, so an import inside a
comment or a string is not one, and an import inside `#if` is. The module is `X`, the first dotted
segment. When `X` is the module name of no target, the import is not a repository import: Foundation, SwiftUI
and every other package are not repository files. It is a package use (see "Package uses"). Otherwise the target is chosen, then its entry file:

1. Among the targets with that module name, the nearest to the importer. A package that contains
   the importer (its `Package.swift` directory is an ancestor of the importing file) beats one that
   does not; then the longer shared leading directory path between the package directory and the
   importer wins; then the package directory first in path order. So a module name declared by
   several manifests resolves to the nearest one.
2. The entry file of the target is, among the known files:
   `<dir>/<name>.swift` for the target name, else `<dir>/<module>.swift` for the module name when it
   differs, else the first `.swift` file below `<dir>` (at any depth) in path order, ignoring a file
   named `Package.swift`. A target with no such file is ignored: its directory may be generated or
   excluded, and that is not an error.

The import is a resolved import of kind `runtime` at the line where the declaration starts (its
`@testable` or `@_exported` attribute included), and uses no symbol. The exception is an import
qualified with a declaration keyword (`struct`, `class`, `enum`, `protocol`, `func`, `var`, `let`,
`typealias`) and a dotted path `X.Y`: it uses the last segment, so `import struct AppCore.Frame`
uses `Frame`. `import X.Y` without a keyword imports a submodule and uses no symbol. Because an
import always lands on the entry file, a node must own the whole target directory: see "Code
units" for why `init` makes one node per target.

### Shell resolution

A Shell file is read with the bash grammar; zsh and dash scripts are read the same way.

**Commands.** `source X` and `. X`: a command whose name is exactly `source` or exactly `.` (so
`./x.sh` and `bash x.sh` are not imports), found anywhere in the file, in functions, conditionals and
loops, but not in comments, strings or here-documents. Only the first argument is the path, later
ones are ignored. The import is located at the line where the command starts, has kind `runtime`
and uses no symbol. The path has a value, or the reference is skipped as `dynamic-specifier`. The
specifier of a skipped or unresolved path is the text of the argument as written, quotes included.

**The value of a path.** A word or string made of parts joined together. A part is

- literal text: a bare word, a single-quoted string or the text of a double-quoted one. Text with a
  backslash is not literal;
- the script file, `$0`, `${0}`, `$BASH_SOURCE`, `${BASH_SOURCE}` and `${BASH_SOURCE[0]}`, only as the
  argument of `dirname` below;
- the script directory: `$(dirname F)` or `` `dirname F` `` with `F` the script file (quoted or not),
  `${0%/*}`, `${BASH_SOURCE%/*}` and `${BASH_SOURCE[0]%/*}`. It is the directory of the importing
  file;
- `$(cd P && pwd)`, also `pwd -P` and `pwd -L`, with `P` a path whose first part is a script
  directory: the directory `P` names;
- `$NAME` or `${NAME}`: the value of the nearest earlier assignment of `NAME` in the same file,
  where earlier is by position and an assignment may sit anywhere (also after `export`, `local`,
  `readonly` and `declare`). Only an assignment whose value is a path starting with a script
  directory (the forms above, optionally followed by literal text such as `/..`) has a value; when
  the nearest earlier assignment is any other, or there is none, the path is dynamic. An older
  assignment is never used instead of the nearest. `ROOT="$(cd "$(dirname "$0")/.." && pwd)"`
  followed by `. "$ROOT/device/lib/x.sh"` is the standard case.

Any other part (`$HOME`, `$f`, `${x:-y}`, a command substitution that is none of the above, an
arithmetic expansion, a `dirname` of anything but the script file) makes the path dynamic. A path
that starts with a script directory is anchored: the importing file's directory with the rest of the
text appended, `.` and `..` segments resolved lexically; a path that leaves the repository is
ignored. A path that is all literal text is a literal path.

**Resolution.** An anchored path is probed once, against the known files. A literal path that starts
with `/` or `~` names something outside the repository and is ignored. Any other literal path is
tried against the directory of the importing file and then against the repository root; the first
base where it finds a file wins. A path that finds no known file is an `unresolved-import`, except
when it names an existing asset (a tracked `.env`, a config file), which is ignored.

**Globs.** A path with `*` or `?` in its literal text is a pattern: matched against every known file
(any language), `*` and `?` staying inside one `/` segment, and every match, in path order, is an
import, except the importing file itself. A pattern that matches nothing is an `unresolved-import`.
A path that holds `[`, `{`, or a segment `**` is dynamic. Quoting is not looked at: a shell never
expands a quoted `*`, but `source "$ROOT/lib/*.sh"` is written to load those files, and that is the
dependency worth seeing.

### Svelte and HTML

Two more source languages carry TypeScript or JavaScript inside markup. Their scripts are resolved,
typed (`kind`, `test_only`), recorded (`symbols`) and skipped exactly like the imports of a
TypeScript or JavaScript file: aliases, workspace packages, package `imports`, assets, probing and
the declared-package check all apply, governed by the config and `package.json` files above the
Svelte or HTML file. Everything outside the script elements is ignored: markup, text, `<style>`, and
text that merely looks like an import.

**Svelte** (`.svelte`, always source). Every `<script ...>` element is read, whatever its
attributes, so the instance script, `context="module"` and the Svelte 5 `module` attribute all
count. Its text, the characters between the end of the start tag and the end tag, is parsed as
TypeScript when the `lang` attribute is `ts` or `typescript` (ASCII case-insensitive) and as
JavaScript otherwise. Lines are lines of the `.svelte` file, not of the script: a statement on the
third line of the file is reported at line 3. A `src` attribute on a Svelte script is not an import.

**HTML** (`.html`, `.htm`). A file is source when its markup holds a `<script` element, a start tag
whose name is `script` (ASCII case-insensitive) outside comments, whatever its `type`, `src` or
content. A file without one is an asset. A file that cannot be read as UTF-8 is an asset too, so the
check cannot fail a scan. These are read, in source order, each at the line where its start tag
begins:

- A script with no `src` whose `type` is absent, empty, `module`, `text/javascript` or
  `application/javascript` (ASCII case-insensitive): its text is parsed as JavaScript, lines being
  lines of the HTML file. Other types (`importmap`, `application/json`, templates) hold no
  imports and are ignored.
- A script with a `src` and one of those types: an import of that path, see below.
- `<link rel="modulepreload" href="...">`, `rel` being a whitespace separated list of tokens
  (ASCII case-insensitive): an import of the `href`.
- `<link rel="..." href="...">` whose `rel` list holds `stylesheet`, `preload`, `prefetch`, `icon`,
  `apple-touch-icon` or `manifest` (and no `modulepreload`): an asset reference to the `href`. It is
  of kind `asset` and uses no symbol, whatever file it finds, so a dependency on the node that owns
  that file as for an asset import (see "Asset files"). Other `<link>` elements (`canonical`,
  `alternate`, ..) are not read.

An import of a path (`src`, `href`) is of kind `runtime` (`asset` for a resource link) and uses no
symbol: it loads the whole file.
Its value is trimmed and resolved as a document-relative URL: `main.js` means `./main.js`, `../x.js`
and `./x.js` are relative to the directory of the HTML file, and a query or fragment is dropped as
for any specifier. The file is probed like every relative path (see "Probing"), and a path that
finds nothing is an `unresolved-import`, or `unresolved-root-path` when root-absolute, for a
resource link as for a script. A remote `href` and an empty one are ignored. A relative `href` of a
resource link that finds no file is ignored too when the path, taken from the Vite root, names a file
below `<root>/public/` (`./icons/a.svg` in `<root>/index.html` for `<root>/public/icons/a.svg`),
where Vite serves it from, as a root-absolute one is.

**Vite root.** A root-absolute specifier is one that starts with a single `/`. In an HTML file, in a
`src` or `href` and in an `import`, `export ... from`, `import()` or `require()` of its inline
scripts, it names a path below the Vite project root. The root of an HTML file is the first of

1. **The Vite config.** The nearest ancestor directory of the HTML file, the file's own directory
   first, that holds a Vite config: a file named `vite.config.js`, `.mjs`, `.ts`, `.cjs`, `.mts` or
   `.cts` (when a directory holds several, the first in that order, which is Vite's own). The root is
   that directory, adjusted by the `root` option of the config when the option can be read, see
   below. A Vite config sits above a nearer `package.json`: `ui/vite.config.ts` makes `ui` the root
   of `ui/index.html` whatever `package.json` files lie around;
2. **The nearest `package.json`.** The nearest ancestor directory, the file's own directory first,
   that holds a `package.json`;
3. **The repository root.**

So `/src/forest-paint.ts` in `dev/index.html` is `<root>/src/forest-paint.ts`.

The `root` option is read from the config with the TypeScript or JavaScript grammar (by extension),
never by running it. The config object is the operand of `export default`, of `module.exports =`, or
the top level `const` an identifier there names, looking through parentheses, `as` and `satisfies`; a
call such as `defineConfig(..)` stands for its first argument that is an object or a function, and a
function stands for the object it returns (an arrow function's expression body, or the first `return`
of its block). Only a `root` key directly in that object counts (`test.root` and the like do not).
Its value is read when it is one of

- a plain string literal, a path relative to the config directory (`'ui'`, `'./ui'`, `'.'`);
- `__dirname` or `import.meta.dirname`, or `path.dirname(fileURLToPath(import.meta.url))` or
  `path.dirname(__filename)`: the config directory;
- `process.cwd()`: the config directory, which is where Vite runs from;
- `path.resolve(D, 'a', 'b')` or `path.join(D, 'a', 'b')` (the callee may be written with or
  without `path.`), where `D` is any of the directory forms of this list or a plain string literal
  and every other argument is a plain string literal: the directory joined with the literals, `..`
  resolved lexically. A literal first argument is relative to the config directory, so
  `path.resolve('engine/wasm/pkg')` with one argument is `engine/wasm/pkg` below it, as Vite's
  working directory is that of the config; an absolute literal is not recognised;
- `fileURLToPath(new URL('<literal>', import.meta.url))`, also written `new URL(..).pathname`: the
  literal joined onto the config directory.

Any other value (a variable, an absolute path, a path that leaves the repository,
an expression driftwall does not recognise) means the config directory, as does a config that
cannot be read or has no `root`. The extraction functions that are given no file system (`extract`)
know no Vite configs, so they use rules 2 and 3.

A root-absolute specifier is probed like a relative path against the root. A file that Vite would
serve from `<root>/public/` (`/logo.svg` for `<root>/public/logo.svg`) is ignored. A root-absolute
specifier that still finds nothing is never dropped: it is skipped with reason
`unresolved-root-path` (see "Skipped references"), whose message names the root that was used, so
a wrong root is visible. Root-absolute specifiers are not special in TypeScript, JavaScript or
Svelte files.

A remote URL is ignored: a specifier that starts with `//` or with a scheme (`https:`, `http:`,
`data:`, `blob:`, ..: a letter, then letters, digits, `+`, `-` or `.`, then `:`). In an HTML file
this holds for `src`, `href` and for the specifiers of inline scripts, which are not skipped as
undeclared packages either.

#### The markup scanner

Svelte and HTML are read by a small scanner, not a parser. It walks the text for `<` and handles:

- comments `<!-- ... -->` (to the first `-->`, or the end of the file when unterminated): nothing
  inside is read, so a commented-out script is ignored. `<!doctype ...>` and `<? ... ?>` are skipped
  to their `>`, end tags are skipped, and a `<` not followed by a letter, `/`, `!` or `?` is text;
- start tags, with attributes in any order and any case of the name: a name, optionally `=` and a
  value written in double quotes, single quotes or unquoted (up to whitespace or `>`). A quoted
  value may span lines and contain `>`. A repeated attribute counts once, the first. In Svelte
  a value may also be a `{ ... }` expression, read to its matching brace with quoted strings inside
  skipped, so `on:click={() => run()}` ends where it should. A tag with no closing `>` ends the scan;
- the raw text elements `<script>` and `<style>`: the content runs to the first `</script` or
  `</style` (ASCII case-insensitive) followed by whitespace, `/` or `>`, or to the end of the file.
  So a string holding `</script>` ends the script as it does in a browser, and a `<script` inside a
  `<style>` is not an element;
- a start tag written `<script ... />` has no content and consumes no text after it, in HTML as in
  Svelte. HTML itself does not treat `/>` on a script as self-closing; driftwall does, because it
  is how such tags are written and the alternative would swallow the rest of the file;
- CRLF and LF line endings: a line is counted at each `\n`, so `\r\n` files report the same lines
  as `\n` files.

It does not handle character references (`&amp;` in a `src` is read as written), `<template>`
content as separate from the document, CDATA sections, `<textarea>` or `<title>` raw text, Svelte
`{...}` expressions in text (`{@html '<script>..</script>'}` is read as a script), conditional
comments, or an unquoted attribute value that contains `>`. A script inside any element, such as
`<svelte:head>`, is found like one at the top level.

### Re-exports

An import of a name through a module that only re-exports it is attributed to the file that
defines the name, so a file inside a directory can be its own node while callers write the
directory's path (`use crate::ui::Tone;` with `pub use tone::Tone;` in `ui/mod.rs` is a dependency
on `ui/tone.rs`). This is a second step after resolution: an import is first resolved as in the
language's section, then each name it uses is followed.

- **What is followed.** A resolved import that uses named symbols (`Client`, `default`) of a target
  file that re-exports one of them from another repository file. The symbol is replaced by the
  file that defines it, under the name it has there, and the same site is recorded once per
  defining file: `use crate::ui::{Tone, Theme}` with both re-exported from different files is two
  imports at one line, each with its own symbols. A name the module defines itself, and a name no
  re-export explains, stays on the module file.
- **Never followed (nothing is invented).** A site that uses no symbol (a module binding: `use
  crate::ui;`, `import ui`, `from pkg import submodule`, a side-effect import) and a site that uses
  `*` (a glob, `import * as ui`, `export *` as an import) stay on the module file. So does a
  re-export statement itself: the re-exporting module keeps its own edge to the file it names
  (`pub use tone::Tone;` is `ui/mod.rs` to `ui/tone.rs`), also when that file re-exports the name
  again. An import kind (`type-only`, `test_only`, `runtime`), the line and the other symbols of
  the site are unchanged.
- **Chains.** A re-export of a re-export is followed to the end. A walk that meets a name it
  already passed (a cycle) follows nothing: the site stays on the module it named.
- **Defined locally wins.** A module that defines a name (a top-level item in Rust, a declaration
  or `export` in TypeScript, a top-level `def`, `class` or assignment in Python) and also
  re-exports it keeps the name: the site stays on that module.
- **Globs.** `pub use x::*;` (Rust), `export * from './x'` (TypeScript and JavaScript) and `from .x
  import *` (Python) re-export every name `x` exports. A name is followed through globs only when
  exactly one glob source of the module can export it. A source can export a name when the file it
  names defines it, re-exports it by name or can export it through globs of its own (a cheap
  check by names, the same tables the walk uses; a macro-generated name is not known). When none
  or several glob sources can, the site stays on the module. A glob never carries `default`.
- **Rust.** The re-exports of a file are its top-level `pub use`, `pub(crate) use`, `pub(super) use`
  and `pub(in ..) use` declarations: `pub use x::Y;`, `pub use x::{Y, Z};`, `pub use x::Y as W;` (a
  caller of `W` is attributed to the file defining `Y`, under the name `Y`). A private `use` is not
  a re-export. A path that resolves to a module file as a whole (`pub use crate::gitlab;`) re-exports
  a module, not an item, and is not followed.
- **TypeScript and JavaScript.** The re-exports of a file are its `export { X } from './x'`,
  `export { X as Y } from './x'`, `export { default } from`, `export { default as Y } from`,
  `export type { X } from`, `export { type X } from` and `export * from './x'`. `export * as ns from`
  names a namespace, a module binding, and is not followed. `import { X } from './a'; export { X }`
  is not read as a re-export. The specifier resolves as any other, so aliases and `index` files work.
- **Python.** The re-exports of an `__init__.py` are its `from .x import Y`, `from .x import Y as
  W` and `from .x import *` statements (relative or absolute). Only `__init__.py` is read: another
  module that imports a name is not a facade. When a star source defines `__all__` as a list or
  tuple of string literals, that list is the names it can export through a glob; otherwise they are
  its public names (no leading underscore). A name that is a submodule is a module binding and is
  not followed.
- **Facades.** Following to the definition is the default. A node that deliberately presents
  other nodes' items as its own (an anti-corruption layer, a wire module) is declared with
  `%% facade <node>[, <node>]`, one or more nodes per line and any number of lines. When an
  importer that is not in a facade node imports a name through a re-export in a file that node
  owns, the site is attributed to that file, and the walk does not go past it: with `%% facade
  ui-lib`, `import { Comment } from '../lib/wire'` is a dependency on `ui-lib` even though
  `wire.ts` re-exports it from `types`. The walk still follows chains up to the facade file, so a
  barrel in front of the facade is followed. The facade's own re-export statements keep their
  edges to the definitions (`ui-lib --> types`), and inside the facade node nothing changes: its
  own files follow re-exports as above. `exports` and `private-symbol` for a name reached through a
  facade are judged at the facade node, since that is where the site now is. A facade node
  whose files re-export nothing is the warning `stale-facade` at its directive's line
  ("`%% facade ui-lib` on line 9 names a node whose files re-export nothing"), so a stale
  declaration is not left behind. A node that is not declared is a contract error with the line.
  A node listed twice is also an error. `render` writes the directive back as one line, nodes
  sorted. `brief` shows `  facade: ui-lib` in its `rules:` block and the JSON `rules` has
  `facades`, a sorted list of node ids, absent when empty. `diff` reports the `%% facade`
  line like every rule directive.
- **Symbols and surfaces.** The recorded symbol of a followed name is its name at the definition,
  and the edge, the `exports` check (`private-symbol`) and `brief` all see the defining file's node
  and that name. The public surface is judged at the definition, not at the re-exporting module: with
  `%% map tokens: src/ui/tone.rs` and `%% exports tokens: Tone`, `use crate::ui::Tone` is allowed and
  `use crate::ui::Other` (defined in `ui/other.rs`, another node) is judged against that node. The
  facade module's own node needs no `exports` line for what it only forwards. An import that stays
  on the module file is judged against the module's node as before.
- **Not read.** Re-exports inside inline `mod` blocks or functions, and dynamic ones (`__getattr__`,
  `globals()`). A conditional one (`#[cfg(..)]`, `if TYPE_CHECKING:`) is read like any other.

### Import location

An import's line is the line where its statement starts, not the line of the specifier string: the
`import` or `export` keyword of a TypeScript or JavaScript statement, the first token of a
`require(..)` or `import(..)` call expression, the `import` or `from` keyword of a Python statement,
and the `use` declaration of Rust (its `pub` when it has one, never the attributes above it). A
statement that spans several lines is one location, its first line. A Rust qualified path written
in code (`crate::gitlab::fetch(..)`, or a path inside a macro argument) is not a statement and keeps
the line of the path itself. A skipped Python `importlib.import_module(..)` call is located at the
call start, and a Rust embed at the start of its macro invocation. In a Svelte or HTML file every
line is a line of that file, not of the script, and a `src` or `href` is located at the start of its
tag (see "Svelte and HTML").

### Import kind

Every resolved import carries a `kind` (`Runtime`, `TypeOnly`, `Embed` or `Asset`, serialized as the
kebab-case strings `runtime`, `type-only`, `embed`, `asset`) and a `test_only` flag. Neither changes which
edges exist: the linter treats every kind and test import like any other. They are recorded so that
callers can decide what a type-only or test import is worth.

`type-only`:

- TypeScript and JavaScript: `import type ...`, `export type ... from`, an import or re-export where
  every specifier is written `type X` (`import { type A, type B } from`), every import in a
  declaration file (`.d.ts`, `.d.mts`, `.d.cts`), every JSDoc `import('x')` type (see "JSDoc type
  imports"), and every `import("x")` written in a TypeScript type (see "Symbols and public
  surfaces"). `import { type A, B }` is `runtime`, as is a
  default or namespace import beside type specifiers, `import {} from`, and `require(..)` or
  `import(..)`.
- Python: an import anywhere inside the body of `if TYPE_CHECKING:` or `if typing.TYPE_CHECKING:`
  (any nesting depth below it). Its `else` and `elif` branches and any other condition are `runtime`.

`test_only`:

- Rust: an import written inside an item or module annotated `#[cfg(test)]`, at any nesting depth
  (`#[cfg(test)] mod tests { fn f() { use crate::a::B; } }`), including a `use` that is itself
  annotated, and inside a module whose body starts with the inner attribute `#![cfg(test)]`. Only
  the exact predicate `test` counts: `cfg(any(test, ..))`, `cfg(all(test, ..))` and `cfg(not(test))`
  do not. An import in a file under a `tests/` directory or named `tests.rs`, `*_test.rs` or `*_tests.rs` is
  `test_only` whatever it is written in. So is an import in a file that a `#[cfg(test)]` module
  declaration pulls in: `#[cfg(test)] mod tests;`, `#[cfg(test)] mod fixtures;`, any name, in any
  file and at any depth (also inside an inline module, and a declaration annotated through its
  module, `#[cfg(test)] mod a { mod b; }`). The file is found through the module-path model, the
  same one that resolves `use crate::..` paths: relative to the declaring file and its inline
  modules, `<name>.rs` or `<name>/mod.rs`. A declaration that finds no known file, and one whose
  attribute is not exactly `cfg(test)` (see above), pulls in nothing. The modules a test file
  declares, with or without an attribute (`mod support;` in `tests.rs`), are test code in turn, to
  any depth. A `#[path = ".."]` declaration is followed, see "Rust resolution". The declaring file is not made test code.
- TypeScript, JavaScript, Svelte and HTML: files named `*.test.*` or `*.spec.*` (a middle name segment
  equal to `test` or `spec`, so `a.test.ts`, `a.spec.tsx`, `a.test.svelte`), or under a `__tests__`,
  `test` or `tests` directory at any depth.
- Python: files named `test_*.py`, `*_test.py` or `conftest.py`, or under a `tests` or `test`
  directory at any depth.
- Swift: files under a directory named `Tests`, `Test`, `tests` or `test` at any depth.
- Shell: files whose name up to the first `.` starts with `test_` or ends with `_test`, or under a
  `tests` or `test` directory at any depth.

`brief` and `neighbors` show the kinds. An edge whose sites are all `type-only`, `embed` or `asset`, or
all `test_only`, is annotated after its import count, `a --> b (3 imports) (type-only)`, one
annotation per such property (order `type-only`, `embed`, `asset`, `test-only`, `alias-to-build-output`). When a property
holds for some sites only, the count is followed by one count per property that occurs, in the same
order: `a --> b (12 imports, 3 type-only, 2 test-only)`. An edge whose sites are all plain runtime
imports shows only `(N imports)`. In JSON every site carries `kind` and `test_only`.
A site that is an alias to a build output (see "Vite aliases") also carries `via`.

Duplicates merge on (location, target) only when their kind and `test_only` agree. Imports that
reach the same file from the same location with different kinds (`import type { A } from './a';
import { b } from './a';` on one line) stay two entries, so a type-only use is never recorded as
runtime or the other way round. Entries are sorted by location, target, symbols, kind, `test_only`.

`asset`: a TypeScript or JavaScript import that resolves to an existing non-source repository file
(`./data.json`, `./style.css`, `../x.svg`, see "Asset files"), and a `new URL('<file>', import.meta.url)` of such a file (see "File references"). It uses no symbol, so it is never
`private-symbol`, it is a dependency on the node that owns the asset, and it is `asset` even inside a
declaration file. As for every import, an asset owned by nobody or by the importer's own node is no
edge.

`embed` by query: a bundler query makes an import of a source file a load of its text or URL, not of
its code. A TypeScript, JavaScript, Svelte or HTML import (also a root-absolute one, see "Svelte and
HTML") whose specifier has a query with a parameter named `raw`, `url` or `inline` (`./a.ts?raw`,
`./a.ts?url&v=2`, `./a.ts?inline`; the parameter name is what precedes any `=`) and that resolves to a
source file is `embed`: a resolved import that uses no symbol, so it is never `private-symbol`, and
still a dependency on that file's node. Any other query, `?worker` and `?sharedworker` included,
leaves the import `runtime`, and so does any query that holds `worker` or `sharedworker` beside
`inline` (`?worker&inline` is a worker). An import of an asset with such a query stays `asset`, and
one that resolves to nothing is as unresolved as without the query.

### Embeds

A Rust `include_str!("..")`, `include_bytes!("..")` or `include!("..")` (also written `std::..`) with
a plain string literal embeds a file. The path is relative to the directory of the file that holds
the macro. When it is a known repository source file, the embed is a resolved import of kind
`embed` that uses no symbol: an embed is not a use of named items, so it is never `private-symbol`,
and surfaces do not restrict embeds. It still counts as a dependency on that file's node, like any
import. The same holds for an asset a tag covers (`.html`, `.css`, `.png`, a font, see "Asset
files"): the embed is a resolved import of kind `embed` and a dependency on the asset's owner, as a
TypeScript `?raw` import is (and as a TypeScript import of the file is an `asset`). An asset's owner
that the contract does not declare is an `undeclared-owner` skipped reference, as for a TypeScript
import. Only a target that is neither a known source file nor an owned asset stays a skipped
reference with reason `embed-non-source`: a missing file, a path that leaves the repository, and a
repository file no `.arch` tag covers (`architecture.mmd` at a root with no tag), which therefore
never creates an owner, an edge or an `undeclared-owner` record, in a fresh `init` or anywhere.

### Package uses

An import of code outside the repository is a package use. It is never an edge, and it is what the
package rules of the contract check (see "Package rules"). Each file records its package uses next
to its resolved imports: `FileImports::packages`, sorted and free of duplicates.

A package use has

- `name`: the package, as the language writes it (below);
- `kind`: `third-party` (code the repository depends on but does not contain) or `stdlib` (what
  the language or platform ships);
- `at`: the file and the line the statement starts on, like any import (see "Import location");
- `test_only`: whether the statement is written in test code (see "Import kind").

Uses of one package on one line merge, so `use serde::{A, B};` is one use. The same package on two
lines is two uses: the count of a package is its number of uses. In JSON a use is flat: `file`,
`line`, `name`, `kind`, `test_only`.

**TypeScript, JavaScript, Svelte and HTML** (every specifier that reaches the package check, step 6
of "TypeScript and JavaScript resolution"). A Node builtin is `stdlib` and is named
`node:<module>` whether it is written `fs`, `node:fs` or `fs/promises`: the first segment after an
optional `node:`, so the subpath is dropped (`node:fs`, `node:child_process`). Any other package is
`third-party`, named as `package_name` names it: `lit`, `@lit/reactive-element`; `three/examples/jsm/x`
is `three`. This holds whether `package.json` declares the dependency or not: an undeclared one is
the skipped reference `undeclared-package` and also a package use, because both are true (the skip
says a dependency is missing, the use says what the import is). Not packages, so no use: relative
specifiers, specifiers a `paths` pattern matches, Vite aliases, workspace packages, package
`imports` keys (`#x`), virtual modules, remote URLs and any specifier that is not a name (a URL
scheme, a leading `/`). `import type` and a JSDoc `import('x')` are uses like any other: the kind of
the import is not part of a package use.

**Python.** An absolute module that no import root resolves and whose top-level name matches no
first-party module or package (the final miss of "Misses"). The name is the top-level module:
`import yaml.constructor` is `yaml`, `import os.path` is `os`. That is the import name, not the
distribution name (`PIL`, not `Pillow`). The kind is `stdlib` when the name is in the embedded
`sys.stdlib_module_names` of **Python 3.13** (290 names, private modules such as `_thread`
included), else `third-party`. A module removed before 3.13 (`distutils`, `imp`, `asyncore`) is
therefore `third-party`; the list is pinned so that output never depends on the interpreter that
happens to run driftwall. A namespace-directory miss (step 1a of "Misses") is a `third-party` use
as well as its skipped reference. `from __future__ import x` is a compiler directive, read by no
query, so no use. An `unresolved-import` (a first-party miss) and an
`unreachable-module` are not packages: the top-level name is first-party.

**Rust.** The first segment of a path, in a `use`, an expression, a type or a macro argument, that
names a crate the nearest `Cargo.toml` above the file lists in `[dependencies]`,
`[dev-dependencies]`, `[build-dependencies]` or a `[target.*]` table of those (the key, so a renamed
dependency is its rename, with `-` written `_` as code imports it), and that is not a library crate
of the repository, which resolves to a file. `std`, `core` and `alloc` are `stdlib`; every crate a
manifest lists is `third-party`. A name no manifest lists (a local variable, a type, an
`extern crate` rename) is no use: a rename misses a use rather than inventing one. The name is the
crate as imported, with underscores: `serde_json`.

**Swift.** `import X`, where `X` is the first segment and is the module of no target a
`Package.swift` declares. `X` is `stdlib` when it is on this closed list of Apple system frameworks
the Swift runtime, the test modules and the manifest modules (`PackageDescription`),
`third-party` otherwise:

```
  AVFAudio AVFoundation AVKit Accelerate Accessibility ActivityKit AppIntents AppKit
  ApplicationServices AudioToolbox AudioUnit AuthenticationServices BackgroundTasks CFNetwork
  CallKit Carbon Charts Cocoa Combine CompilerPluginSupport Contacts CoreAudio CoreAudioKit
  CoreBluetooth CoreData CoreFoundation CoreGraphics CoreHaptics CoreImage CoreLocation CoreMIDI
  CoreML CoreMedia CoreMotion CoreServices CoreSpotlight CoreText CoreTransferable CoreVideo
  CryptoKit Darwin DeviceCheck Dispatch Distributed EventKit ExternalAccessory Foundation
  FoundationEssentials FoundationNetworking FoundationXML GLKit GameController GameKit GameplayKit
  Glibc GroupActivities HealthKit IOKit IOSurface ImageIO Intents LinkPresentation
  LocalAuthentication MapKit MediaPlayer MessageUI Messages Metal MetalKit MetalPerformanceShaders
  ModelIO MultipeerConnectivity NaturalLanguage Network NetworkExtension NotificationCenter OSLog
  ObjectiveC Observation OpenGL PDFKit PackageDescription PassKit PencilKit Photos PhotosUI
  PushKit QuartzCore QuickLook RealityKit RegexBuilder ReplayKit SafariServices SceneKit
  ScreenCaptureKit Security ServiceManagement Speech SpriteKit StoreKit Swift SwiftData SwiftUI
  SystemConfiguration Testing TipKit Translation UIKit UniformTypeIdentifiers UserNotifications
  VideoToolbox Vision WebKit WidgetKit XCTest _Concurrency _StringProcessing os simd
```

A module a target declares is a repository module and never a package, even when its entry file is
missing.

**Shell.** None: `source` reads repository files only.

**Skipped references stay as they are.** A skipped reference means driftwall could not follow a
reference; a package use means it knows what the reference is. No reason changed meaning, and
nothing that is a package use is reported under a reason that says otherwise.
`undeclared-package` is both: every such reference is also a `third-party` use, which is what the
reason has always said. Every other reason (`dynamic-specifier`, `unreachable-module`,
`alias-target-missing`, ...) is a reference whose target is unknown or first-party, so it is no
package use. A declared third party package or a builtin, which `skipped` never listed, is a package
use and still not skipped.

### Skipped references

A reference driftwall cannot follow is never silently dropped. `FileImports.skipped` lists each as
a location, the specifier as written and a typed reason (kebab-case in serialization):

| Reason | When |
| --- | --- |
| `undeclared-package` | TypeScript or JavaScript specifier that is not relative, matches no `tsconfig` or `jsconfig` `paths` alias, is not a workspace package, is not a Node builtin and is not a declared dependency; a Python absolute import that misses below a namespace directory, a top-level directory with no `__init__.py` (see "Python resolution") |
| `dynamic-specifier` | a shell `source` or `.` whose path has no value, see "Shell resolution"; `import(expr)` or `require(expr)`, or `new URL(expr, import.meta.url)`, whose argument is not a string literal or a template without substitutions; Python `importlib.import_module(..)` and `__import__(..)` whose first argument is not a plain string literal, or whose relative literal has no usable package, and a Python file loader whose path is not a literal naming a known file (see "Python resolution") |
| `embed-non-source` | a Rust embed whose target is neither a known source file nor an asset a tag covers |
| `unreachable-module` | a Python absolute import that no import root resolves, whose top-level name matches a first-party module or package elsewhere in the repository (see "Python resolution") |
| `alias-target-missing` | a TypeScript or JavaScript specifier that a Vite `resolve.alias` entry matches whose target path has no file in the repository (see "Vite aliases") |
| `undeclared-owner` | an import of an asset or an excluded file whose owner is not a declared node, recorded by `lint` rather than by import extraction (see "Asset files" and "Excluded files") |
| `unresolved-root-path` | a root-absolute `src`, `href` or import specifier of an HTML file that finds no file below its Vite root (see "Svelte and HTML") |
| `file-reference-missing` | a `new URL('<literal>', import.meta.url)` whose target is a directory, missing or outside the repository (see "File references") |

A Vite virtual module is ignored, not skipped: a specifier that starts with `virtual:` or with the
character U+0000 (Rollup's prefix for modules a plugin generates, `virtual:pwa-register`) has no
repository file and no package to declare, so it is neither `undeclared-package` nor any other
skipped reason, and no census count. It is checked last, so an alias, a workspace package or a
`paths` pattern with that name still resolves first.

For `undeclared-package`, a specifier that a `paths` pattern matches is an alias and never skipped,
even when no file backs it (that stays silent, as before). `baseUrl` alone is not an alias: a bare
specifier it cannot resolve falls through to the package check. A builtin is a `node:` specifier or
one whose first path segment is a Node builtin module name (`fs`, `path`, `fs/promises`, ..). A
declared dependency is a name listed in `dependencies`, `devDependencies`, `peerDependencies` or
`optionalDependencies` of any `package.json` at or above the file (see "Packages"); `@a/b/sub` is the
package `@a/b` and `pkg/sub` is `pkg`. A file with no readable `package.json` above it declares nothing.
A malformed `package.json` is skipped silently, as a malformed `tsconfig` is, and the others still
count. Declared third party packages are never
skipped: they are not repository imports and nothing is wrong (they are package uses). `skipped` is not a lint finding.
For a skipped Python call the specifier is the text of its first argument.

A skipped reference may carry a `message`, a sentence of explanation. `unreachable-module` has one:
it names the repository path the top-level name matches. So has `unresolved-root-path`: it names
the Vite root that was searched. So has `alias-target-missing`: it names the alias and its target, and
the node whose directory holds the target when the report knows the owners. And `undeclared-owner`: it names the asset or excluded file and its owner. `message` is absent from the
serialized form when there is none.

## Symbols and public surfaces

Every resolved import records the symbols it uses, exactly as written in the code, never inferred:

- Rust: names in `use` trees (`use crate::gitlab::{Client, fetch}` uses `Client`, `fetch`), the first
  item segment of a qualified path (`crate::gitlab::fetch_mrs(..)` uses `fetch_mrs`), and paths that
  start with a name the same file brought into scope with `use` or declared with `mod`
  (`use crate::gitlab;` then `gitlab::Client` uses `Client`). A glob (`use crate::gitlab::*;`) uses
  `*`. A module binding (`use crate::gitlab;`, `use crate::gitlab as gl;` or `self` in a group,
  `use crate::gitlab::{self, Client}`) uses no symbol on its own line: it is a dependency on the module file,
  and the items reached through its name are recorded at their own sites.
- TypeScript and JavaScript: named imports and re-exports by their exported name, `default` for a
  default import, `*` for namespace imports, `require`, dynamic `import()` and `export *`. A
  `require(..)` or `import(..)` (awaited or not) that is the whole value of a declarator whose name
  is an object pattern uses the exported names the pattern lists: `const { a, b: c } = require('x')`
  uses `a` and `b`, and `const { a } = await import('x')` uses `a`. A rest element or a computed key
  in the pattern adds `*`, since the names it takes are not written. Any other `require(..)` or
  `import(..)` (`const m = require('x')`, `require('x').y`, an array pattern) uses `*`.
  An `import("x")` written in a type is `type-only` and uses the member written right after it,
  `import("x").A.B` uses `A`, and `typeof import("x")` uses `*`. It is in a type when it is
  below a type annotation (`let v: import("x").T`, a field or parameter), a `typeof` type
  query, type arguments, an `extends` or `implements` clause, the value of a type alias, or the
  type of `as` or `satisfies`; an `import()` in an expression is a dynamic import, whatever
  surrounds it. Its location is the line of the `import` word.
- Python: `from a import b, c` uses `b`, `c` when they resolve to names inside `a`. `from a import *`
  uses `*`. A module binding (`import a.b`, `import a.b as c`, `from a import submodule`, a literal
  `importlib.import_module("a.b")`) uses no symbol, as in Rust: it is a dependency on the module file.
- A side-effect import (`import './x'`) uses no symbol.
- A name that a module re-exports is recorded at the file that defines it, under the name it has
  there, see "Re-exports".

Method calls on values (`client.fetch()`) are not resolved: knowing which `fetch` needs type
information. driftwall reports only what the syntax proves.

A node may declare its public surface in the contract with a directive comment, which Mermaid
renderers ignore:

```
%% exports gitlab: Client, fetch_mrs, post_note
```

At most one `exports` line per node, naming a declared node (checked after the whole file is read,
like edge endpoints). The list is one or more comma separated identifiers (`[A-Za-z_$]` then
letters, digits, `_` or `$`), each at most once. A comment whose first word is `exports` is always
this directive, so a malformed one (no `:`, empty list, bad or repeated name) is a hard error with
its line, never a silent comment; every other `%%` line stays a plain comment. The renderer writes
the lines after the edges, sorted by node, names sorted.

When a node has a surface, an import from another node (both owners declared, so unmapped and unknown
owners never cascade) that uses a symbol outside it, or uses `*`, is a `private-symbol` error at the
import, one per symbol. An import that uses no symbol (side-effect) is never private, and imports
inside one node are never checked. Nodes without an `exports` line are unrestricted.

`*` is always a violation for a node with a surface: the importer reaches names that are never seen,
so it cannot be checked. A Rust glob (`use crate::gitlab::*;`), TypeScript `import * as` and
`export *`, and Python `from a import *` all record `*`. A Rust or Python module binding records
nothing, so `use crate::gitlab;` is not private by itself, and each `gitlab::item` used through it is
checked against the surface at its own line, one per item. Python records no such item sites: a
Python module import is not checked against a surface.

`diff` reports surface changes as `+ exports gitlab: X` and `- exports gitlab: Y`, one line per
name; a node that loses its whole surface shows only `-` lines and is unrestricted from then on.
`brief` and `neighbors` list the symbols each edge uses (`uses: Client, fetch`, the first six then
`+N more`) and mark a declared surface (`exports: ...`); JSON carries `symbols` on every edge and
`exports` on a node that has a surface.

An import that uses no symbol is a use of the module itself (`use crate::gitlab;`, `from x import
sub`, `import a.b`, a side-effect `import './x'`): it is displayed as `module`, listed first in
`uses:` (`uses: module, Client`), and an edge with such a site has `"module": true` in JSON. It is
never displayed as `*`, which stays a glob or namespace import. An embed or an asset import uses no
module and no symbol and adds nothing to `uses:`.

## Rules (violation kinds)

| Kind | Severity | When |
| --- | --- | --- |
| `unmapped-file` | error | source file has no inherited `.arch` tag (no matching `%% map` glob, with a map) |
| `unknown-node` | error | `.arch` names a node the diagram does not declare |
| `undeclared-edge` | error | code imports across nodes and the diagram has no such edge (an `%% order open` chain allows the downward ones it has not narrowed, see "Open order") |
| `stale-edge` | warning | solid edge with no import backing it |
| `planned-realized` | warning | dotted edge now backed by production code, promote it to solid (a dotted test or type edge to `-->|test|` or `-->|type|`) |
| `redundant-edge` | warning | node edge that a boundary edge already covers with a kind at least as wide, see "Boundary edges" |
| `test-only-edge` | warning | solid edge whose every import site is test-only, declare it `-->|test|`; also an unlabelled dotted edge whose every site is test-only, which the app does not use yet |
| `stale-facade` | warning | a node named by `%% facade` owns no file that re-exports anything, see "Re-exports" |
| `unresolved-import` | warning | relative import resolves to nothing, whatever its extension; Python absolute import into a first-party package that holds no such module; shell `source` of a path that finds no file |
| `private-symbol` | error | an import uses a symbol outside the target node's declared surface |
| `over-budget` | warning | a node exceeds a `%% budget` limit (fan-out, files or lines), see "Budgets" |
| `production-imports-test` | error | a production import site whose target is a test file, with `%% tests: isolated`, see "Test isolation" |
| `isolation-breach` | error | an import goes between two children of a `%% isolated` boundary, see "Isolated boundaries" |
| `stale-isolation` | warning | a `%% isolated` boundary has fewer than two children, see "Isolated boundaries" |
| `upward-edge` | error | an import goes from a node in a lower `%% order` boundary to one in a higher boundary, see "Layer order" |
| `forbidden-package` | error | a non-test package use that a `%% forbid-packages` line names, or a third-party one a `%% packages` allow-list of its node lacks, see "Package rules" |
| `stale-map` | warning | a `%% map` glob matches no file, see "Contract map" |
| `stale-package` | warning | an entry of a `%% packages` line that no non-test package use of the node matches |
| `ignored-tag-files` | warning | the contract has `%% map` lines and `.arch` files exist, which are not read |
| `stale-baseline` | warning | a debt baseline entry records more findings than exist now, or a key that no longer occurs, see "Debt baseline" |

A boundary edge permits the import of every pair it covers, by the rules of its kind (see "Boundary
edges"). A dotted edge permits the import. `--strict` turns every warning into a failure. A test edge permits
only test-only imports (see "Test edges"): a production import on it is `undeclared-edge`. A type edge
permits only type-only imports (see "Type edges"): a runtime import on it is `undeclared-edge`.

## Debt baseline

A designed contract leaves existing debt undeclared, so `lint` shows it. A repository that starts
with nine errors cannot run `lint` in CI until all nine are fixed. The debt baseline records today's
findings once, so CI fails only on new debt, fixing debt never fails, and the record can only
shrink unless a human deliberately grows it.

```
driftwall lint [PATH] --write-baseline <FILE>
driftwall lint [PATH] --baseline <FILE> [--strict]
driftwall diff --base <REF> [PATH] --baseline <FILE> [--exit-code]
```

`--write-baseline` and `--baseline` are mutually exclusive on `lint`: passing both is a usage
error, exit `2`. Both paths are relative to `PATH`, like `--contract`.

### File format

Plain text, written by `--write-baseline`:

```
# driftwall debt baseline. Each line is one kind of existing finding: <kind> <count> <key>.
# CI fails when a key has more findings than recorded, or a new key appears.
# Fixing an entry never fails CI; `lint --strict` then asks to shrink the file with
# `driftwall lint --write-baseline <file>`.
undeclared-edge 2 billing-service --> search-service
undeclared-edge 1 billing-service --> search-service (test)
unmapped-file 1 src/legacy/tool.rs
```

- One entry per line, `<kind> <count> <key>`, separated by single spaces; the key is the rest of
  the line. `count` is a positive integer. Blank lines and lines starting with `#` are ignored.
- Entries are sorted by kind name, then by key (byte order), so writing is deterministic and
  idempotent: the same findings give byte-identical output. The header is fixed text.
- A malformed line (an unknown kind, a count that is not a positive integer, a missing key, a
  duplicate `<kind> <key>`) is a tool error, exit `2`, naming the file and the line.
- Writing prints `driftwall: wrote <FILE> (<n> entries, <m> findings)` and exits `0`, whatever the
  findings were. Contract errors (a contract that does not parse) are tool errors and are never
  baselined. `stale-baseline` is never recorded.

### Keys

A key holds nothing that changes when unrelated code is edited: no line numbers, no message text, no
counts. A per-site kind counts its sites: the count of an entry is the number of findings with that
key. A finding at a test-only import site (see "Import kind") has ` (test)` appended to its key, so
test debt and production debt are separate entries. Paths use `/`.

| Kind | Key | Count is |
| --- | --- | --- |
| `unmapped-file` | `<file>` | 1 |
| `unknown-node` | `<tag file> <node>` | tags in that file naming it |
| `undeclared-edge` | `<from> --> <to>` | import sites |
| `stale-edge` | `<from> <arrow> <to>`, arrow as declared | 1 |
| `planned-realized` | `<from> <arrow> <to>` | 1 |
| `test-only-edge` | `<from> <arrow> <to>` | 1 |
| `redundant-edge` | `<from> <arrow> <to>` | 1 |
| `unresolved-import` | `<file> <specifier>` | import sites |
| `private-symbol` | `<from> --> <to> <symbol>` (`*` for a glob import) | import sites |
| `stale-map` | `<glob>` | 1 |
| `ignored-tag-files` | `<contract file>` | 1 |
| `upward-edge` | `<from> --> <to>` | import sites |
| `over-budget` | `<node> <measure>` | 1: the limit is in the contract, not the baseline |
| `production-imports-test` | `<file> --> <test file>` | import sites |
| `forbidden-package` | `<node> <package>` | uses |
| `stale-package` | `<node> <entry>` | 1 |
| `stale-facade` | `<node>` | 1 |
| `isolation-breach` | `<boundary>: <from> --> <to>` | import sites |
| `stale-isolation` | `<boundary>` | 1 |

An edge finding of an `%% each` template uses the template's roles and starts with
`each <boundary>: `. `stale-baseline` has no key. `upward-edge` is keyed by the edge, without a
symbol: one import is one finding however many names it brings, so a key per name would change when
a name is added to an existing import.

### Applying a baseline

`lint --baseline <FILE>` compares the findings with the file by key:

- A key with at most the recorded count of findings is suppressed: its findings are not printed,
  and they decide neither the exit code nor `--strict`.
- A key with more findings than recorded is reported in full, every finding of it, with
  `; the baseline allows <N>, found <M>` appended to the message: the tool cannot tell which site is
  new, and naming all of them is honest. In the text output the suffix is shown once per group, see "Grouped findings". A key the file lacks is reported as it is, with no suffix.
- The text summary gains `, K baselined` (`driftwall: clean, 9 baselined`,
  `driftwall: 1 error, 0 warnings, 9 baselined`); without `--baseline` it is unchanged.
- JSON omits suppressed findings from `violations` and has a top-level `baseline` object
  `{"file", "suppressed", "stale"}` after `summary`; it is absent without `--baseline`. SARIF emits
  suppressed findings too, each with `"suppressions": [{"kind": "external"}]`, the SARIF way to say a
  tool-external file accepted it.
- A missing or unreadable baseline file is a tool error (exit `2`), like a missing contract.

### The ratchet

An entry whose recorded count is above the number of findings now, or whose key no longer occurs, is
the warning `stale-baseline`, located at the baseline file and the entry's line, with the hint to
run `driftwall lint --write-baseline <FILE>` to shrink it. It is a warning, so a plain run still
passes and `--strict` fails until the file shrinks. The file therefore never silently keeps room for
debt to return.

### Growth is visible

`driftwall diff --base <REF> --baseline <FILE>` also compares the baseline at `REF` with the working
tree one (read like the contract: `git show REF:./<FILE>`). An entry that is new, or whose count
went up, is `debt added`; one that went down or vanished is not reported. A file absent at `REF`
counts as empty, so every entry is added. The text output has a `debt added:` section, one
`<kind> <key>: <old> -> <new>` line per entry, after the contract changes; the JSON has `debt_added`,
a list of `{kind, key, old, new}`, present only with `--baseline`. `--exit-code` exits `1` when the
contract changed or debt was added. A baseline file missing in the working tree is a tool error.

## init

`driftwall init` reverse-engineers a baseline from an existing repository so that `driftwall lint`
on the result is clean at once (with the same `--contract`). It writes `architecture.mmd` and the
`.arch` tags (a contract without tags would leave every file unmapped); `--no-tags` writes the
contract only.

Source files are discovered like `lint` does: ignores are honored, vendored, minified and generated files are
excluded (see "Excluded files"), and no source files is an error. `init` decides, in this order,
which node every source file belongs to. A file is decided by the first rule that covers it.

1. **Existing tags.** Every file that already has an owner through an `.arch` file keeps it, with
   or without `--no-tags`, with or without `--force`. These are fixed decisions: `init` plans only
   for the files that have no owner, never rewrites or removes an `.arch` file it did not just
   create, and declares in the contract every node the existing tags name (also a tag whose
   directory holds no source). A hand written `src/.arch` with `node: *` therefore steers `init`.
   An existing `.arch` below a directory `init` tags is fine: the nearer tag wins, as everywhere.
   `--force` only lets `init` overwrite files it writes itself: the contract, and an `.arch` at a
   path it plans for that exists but that the scan did not see (a gitignored one). An existing
   `scope: here` tag covers only its own directory, so the files below it are planned like any
   others, and a plan that needs an `.arch` at that same path is a conflict as for any existing file.
2. **Code units** (only without `--depth`). Directories that a manifest or the layout names as one
   buildable or importable unit are decomposed into nodes, see below.
3. **The automatic cut** (only without `--depth`) for everything that is left. Large nodes that
   result from rules 2 and 3 are then cut once more, see "The layer split".
4. **`--depth N`** replaces rules 2 and 3: every file left belongs to its ancestor directory at
   depth N from the repo root, or to its own directory when that is shallower, one directory node
   each. Module nodes and units never appear under an explicit `--depth`, so both layouts stay
   reachable without a flag.

### Code units

A unit is a directory. Units are found from manifests and the file layout:

- **Rust crate**: a directory whose `Cargo.toml` has a `[package]` and that holds a `src/`
  directory; and every directory a `[workspace] members` list of any `Cargo.toml` names that holds
  a `Cargo.toml` (members are relative to that manifest, `exclude` removes directories). A member
  entry may use `*` and `?` inside a path segment (`crates/*`, `crates/app-?`), expanded against
  the directories that exist. Other glob syntax (`**`, `[a-z]`) and entries that leave the
  workspace (`..`) match nothing.
- **JavaScript unit**: every directory that holds a `package.json`, a `tsconfig.json` or a
  `jsconfig.json`, the repository root included. Workspace members are directories with a
  `package.json`, so they are units without their `workspaces` list being read (pnpm and lerna
  workspace files are not read either).
- **Python package**: a directory with an `__init__.py` that has no ancestor directory with an
  `__init__.py` (the outermost package; sub-packages belong to it).
- **Swift target**: the directory of every target a `Package.swift` declares, see "Swift
  resolution". The repository root is never a Swift target (a target with `path: "."` is not a
  unit), and manifests are read only in directories that hold, or lie above, a source file, like the
  others.

Manifests are read only in directories that hold, or lie above, a source file, so ignored trees are
never consulted. A `Cargo.toml` that cannot be read or parsed is skipped, like a broken one is for
import resolution. The existence of a `package.json`, `tsconfig.json` or `jsconfig.json` is enough
for a JavaScript unit: their contents are not read. A unit with no unowned source file is dropped.
A directory found by two rules is one unit, of the first kind in the order Rust crate, JavaScript
unit, Python package, Swift target. Two Swift targets that share a directory are one unit, named
after the first target in name order.

**Every unit is decomposed, except a Swift target.** A file belongs to the unit of its deepest ancestor unit directory.
Each unit has a source directory: a crate's `src/`; a JavaScript unit's `src/` when it holds a
source file there, else the unit directory; a Python package's own directory. How the source
directory is cut depends on the unit:

- **Module nodes: a Rust crate, a Python package, and the primary JavaScript unit.** Their files
  are modules. When the source directory directly holds two or more modules (source files and
  subdirectories both count, by distinct derived name: `a.ts` and `a.test.ts` are one, and so are
  `a.rs` and `a/`; only files of that unit count), it is tagged `node: *`
  (see "Prefixes"), so every module and subdirectory of it is a node named by the module rule.
  When a derived id is not valid, is reserved or is already taken (see "Ids"), the source
  directory is not decomposed. A folded entry file (see "Module nodes") counts for no module and
  owns no node, so a package of an empty `__init__.py` and one module is one directory node, and a
  crate whose `lib.rs` only declares modules has no `lib` node. The primary JavaScript unit is also not decomposed when its source
  directory is the repository root and any source file of the repository lies in a subdirectory
  (see "Root files").
- **Directory nodes: every other JavaScript unit.** A JavaScript or TypeScript unit that is not the
  primary unit, including one declared only by a `tsconfig.json` or `jsconfig.json` and one that
  lies inside another unit's source directory, is a layer of the project and not a set of file
  modules, so its source directory is never `node: *`. It is split by subdirectory only: the files
  directly in the source directory are one group node (tagged `scope: here`, see "Contract and
  writing"), and each subdirectory of the source directory that holds source files of the unit is
  one node covering everything below it that no nearer tag or unit takes. A file added to a
  subdirectory stays in its node; a file added to the source directory joins the group. Every such
  unit is split, whatever it holds.
- **The rest of the unit.** When the source directory is a subdirectory of the unit, the other files
  of the unit are directory nodes too: each subdirectory of the unit directory other than the
  source directory (`test/`, `scripts/`, `dev/`) is one node covering everything below it that no
  nearer tag or unit takes, and the files directly in the unit directory are one node of the unit
  directory (the unit group, tagged `scope: here`, see "Contract and writing").
- **A unit that is not decomposed.** The primary unit (below) decides nothing: its files are cut
  by rule 3 like any other. Every other crate or package is one directory node covering its whole
  directory, where a nested unit and nearer tags still win.

- **A Swift target is one node, never decomposed.** Files of a Swift module never import each other
  and every import of the module resolves to a single entry file (see "Swift resolution"), so a
  `node: *` cut would put each file in a node of its own: the edges to the module would land on the
  entry file's node and every other file of the module would be a node nothing points at. The whole
  target directory is one directory node, tagged with a plain `node:`, and its id is the target
  name sanitised (see "Ids"), without the prefix of any unit. A Swift target is never the primary
  unit and never counts when the primary unit is chosen. Where a Swift target lies inside another
  unit (a `package.json` above `Sources/`), its own files belong to the target, as for every nested
  unit.

A unit that sits inside another unit takes its own files, so a root `package.json` next to a
sub-project `ui/` with its own `package.json` is two units. A sole unit is decomposed like any
other: a lone JavaScript or Python unit gets module nodes like a lone crate does.

The **primary unit** is the unit at the repository root, or else the only unit. It has no prefix;
every other unit has the prefix `<unit-id>-`, where the unit id is the unit directory's path with
its segments joined by `-` and sanitised as an id (see "Ids"): `ui/` is `ui-`, `engine/core` is
`engine-core-`. So `ui/src/components/` becomes the node `ui-components` when `ui/` is not primary,
and `components` when it is.

### The automatic cut

- Starting at the root, descend while the directory holds no source file itself and exactly one
  child directory holds sources. The directory where that stops (own source files, or two or more
  source-bearing children) is the fork; N is its depth plus one. So code under `src/` alone yields
  `src/auth` and `src/billing`, not one `src` node, while a repo with one root module plus `lib/`
  yields `root` and `lib`, and one directory holding one module yields a single node.
- When the fork directly holds two or more modules (here only source files directly in the fork
  count, not subdirectories: a fork with one file and two subdirectories keeps directory nodes,
  unlike a unit's source directory), it gets `node: *` and every module and
  subdirectory is a node named by the module rule, so contract ids are exactly the ids the linter
  derives, labelled `<fork>/<id>`. A fork holding one module keeps directory nodes. When any file
  under the fork would derive an id the contract cannot declare (`.eslintrc.js`, `end.rs`) or an id
  that is already taken, `init` falls back to directory nodes instead of writing a baseline that
  cannot lint.
- **Root files.** The repository root is never a `node: *` source root when it also has source
  directories: its own files (`vite.config.ts`, `eslint.config.js`, `build.rs`, `setup.py`) are
  tooling next to the code, and become one node `root` (tagged `scope: here`) beside one node per
  source directory. A root that holds only files (flat scripts) still gets `node: *`. A project
  that wants root modules writes a root `.arch` with `node: *` itself (rule 1). This holds when
  the root is the source directory of the primary JavaScript unit too: with a root `package.json`
  whose `src/` is itself a unit (it has a `tsconfig.json`), the files of the root unit are the
  tooling files and the directories beside `src/`, so they are cut here and not by `node: *`.

### The layer split

After rules 2 and 3 (never under `--depth`), a node that is too big to read is cut one more level.
A node qualifies when it stands for one directory (a directory node, or a module node whose files
all lie below one subdirectory of a `node: *` directory), owns at least 35 source files, and at
least one child directory of it holds at least 8 of those files. Then every child directory with
at least 8 of the node's files becomes a directory node `<id>-<child>` covering everything below it
that no nearer tag takes, and the node keeps the rest (its loose files and the small child
directories) as a directory node of the directory, with the same id it had and scope `subtree`
(small children are not covered by a tag of their own, so they must stay under the parent). A
module node that is split is no longer derived by `node: *`: its id is now declared by the tag at its
directory, and the nearer tags win. When the node has a name (`ui-components`, a Swift target) the
children are `<name>-<child>`; otherwise their ids come from the path like any directory node.

A node with more than 4 qualifying children is not split: that is a flat collection of features
(`src/ui` with `kit/`, `lesson/`, `mascot/`, ..), not layers, and splitting it would only multiply
nodes. The split applies once, to the nodes rules 2 and 3 produced, never to its own results. The
numbers 35, 8 and 4 were chosen on real repositories: they split a screens-plus-`settings/`-plus-`ui/`
directory of 38 files and leave a feature collection of 195 files, and 33-file directories, whole.

### Prefixes

`init` writes `node: *` with no prefix for the primary unit and for the automatic cut, and
`node: <unit-id>-*` for every other Rust crate and Python package. The prefix is part of the derived
id: a prefixed module is "taken" when `<prefix><name>` is. A non-primary JavaScript unit is not
tagged with a module prefix (it has directory nodes), but its subdirectory nodes are named with the
same prefix, see "Ids".

### Ids

- Directory node ids (units, directory cuts, the root node) come from the directory path below the
  longest prefix all new directory nodes share, segments joined with `-`, other characters replaced
  by `-` (repeats collapsed, ends trimmed, `n-` prepended to a non-letter start). A directory that
  is the shared prefix itself is named by its last segment, and the root directory is `root`. So
  `engine/core` and `engine/cli` next to `src` are `engine-core`, `engine-cli` and `src`. The
  directories of a unit that has a prefix (see "Prefixes") are the exception: they use their whole
  path, which does not count for the shared prefix, so the `test/` directory of the unit `ui/` is
  `ui-test`, in step with its module ids `ui-...`, and the group of the unit `ui/` is `ui`.
- A subdirectory of the source directory of a non-primary JavaScript unit is named like the module
  of that name: the unit prefix followed by the directory name, sanitised as above, so
  `ui/src/components/` is `ui-components` (not `ui-src-components`). The loose files of the source
  directory are named by the whole path, so `ui/src` is `ui-src`; the unit group is the unit id
  (`ui`), and a directory of the unit outside the source directory uses its whole path (`ui/dev` is
  `ui-dev`). The normal collision rules apply when two of these are the same id.
- A Swift target directory is named by its target name: the name sanitised as above replaces the
  path derivation, and the directory does not count for the shared prefix of the others. The same
  collision rules as for any directory node apply.
- **A module id never collides with another directory's id family.** A plain `node: *` (no prefix)
  that `init` is about to write is written as `node: <dir>-*`, `<dir>` being the sanitised name of the
  tagged directory, when any module id it derives equals, or is a dash-prefix of, an id of another
  directory: the id of a unit that has a prefix (`engine-core` for the crate `engine/core`, whose
  modules are `engine-core-*`) or a fixed id (so `engine` is a prefix of `engine-core-anim`). The rule
  is applied to the whole tag, because a prefix is a property of the tag: `src/engine.ts` and
  `src/engine/` become `src-engine` along with every other module of `src/`. A tag at the repository
  root has no directory name and keeps its ids. A repository with no such collision gets the same ids
  as before.
- Module ids (`node: *`) are fixed by the module rule and are never renamed. Node ids named by
  existing tags are fixed too. Two existing tags may name the same node on purpose (that is
  grouping); `init` itself never merges two units silently.
- A directory node whose id is a fixed one (module, or existing tag) is named `<id>-dir`, then
  `<id>-dir-2`, `<id>-dir-3`, so a module `src/ui.rs` (node `ui`) and a directory `ui/` give `ui` and
  `ui-dir`. Any other collision between directory nodes, and the reserved words `end`, `flowchart`
  and `subgraph`, get `-2`, `-3`, ... in sorted directory order. This collision handling is a
  backstop: prefixes keep the ids of different units apart in the ordinary case.
- A `node: *` region (rule 2 or 3) whose derived ids are not valid, or collide with a fixed id or
  with an earlier region's ids, is not created; its files get directory nodes instead.
- **Labels name paths that exist.** A directory node is labelled with its directory (`.` for the
  repository root). A module of a `node: *` directory is labelled `<directory>/<name>`, the path
  of its file without extension (`src/cli` for `src/cli.ts`) or its subdirectory, except the
  module that only test files make (see "Module nodes"): it has no file or directory of its own,
  so it is labelled with the directory (`src`, not `src/src`). A node that only an asset or an
  excluded file names is labelled with the subdirectory below the tag that holds it, or with the
  asset's own path when it lies directly in the directory (`src/index.css`, not `src/index`). The
  same labels come from existing tags (`Existing`). A test asserts that every label `init` writes
  names an existing path, or is `<id>: <description>` (see "The draft reads as a design"), in which
  case `brief` names the node's paths.

### The draft reads as a design

The contract `init` writes is a draft of an architecture, not an inventory. Everything below uses
only what the code and its manifests state; no rule guesses a layer or a purpose, and the design
choices stay with humans and agents. None of it changes what `lint` checks: it is presentation
(boundaries carry no rules, see "Contract format"), so `init` followed by `lint --strict` stays
clean, and the nodes, tags and edges are those of the sections above. Output is byte-identical
across runs.

**Boundaries (subgraphs).** Each node has a *home* directory: the directory of the tag that owns
its files for a module of a `node: *` tag, and the parent of the tag's directory for any other
node. A node also lies in the code unit (see "Code units") that is the deepest ancestor of its
files. Nodes are grouped one level deep, by the first rule that applies:

1. a node of a unit that has fewer than 8 nodes is grouped with the unit's other nodes, under the
   unit's directory;
2. every other node, including every node of a unit with 8 or more nodes and every node outside a
   unit, is grouped with the nodes that have the same home directory.

A group is emitted as a `subgraph` only when it has at least two nodes, is not the repository root
directory, and does not hold every node of the contract (a boundary around everything says
nothing). Other nodes stay at the top level. A boundary id comes from its directory like a
directory node id (see "Ids") and never equals a node id or another boundary: on a collision it
gets the suffix `-group`, then `-group-2`, ... A boundary is rendered with its id as its label.

**Labels with a responsibility.** A node is labelled `<id>: <description>` when the code states one,
taken in this order from the entry file of the node: the first sentence of the leading `//!` inner
doc of a Rust file; the first line of the module docstring of a Python file; the first sentence of
the leading JSDoc block of a TypeScript or JavaScript file, from its `@file`, `@module` or
`@fileoverview` tag, else its first text line (a block with `@license` or `@copyright` is not a
description). The entry file is, among the files of the node at their smallest depth, the one named
`mod`, `lib`, `main`, `__init__` or `index` (in that order), else the only module they make
(`a.ts` and `a.test.ts` are one). A node that stands for a whole unit directory (a directory node
tagged at the unit directory with scope `subtree`) falls back to the `description` of the
`Cargo.toml` `[package]`, `package.json` or `pyproject.toml` there. The description is trimmed to 60
characters at a word boundary (with `…` when cut), backticks and brackets are dropped, and quotes
become `'`. A node with no description keeps the path label of "Ids". The path is then not in the
label, so `brief` names it (the paths of each node).

**Layers.** Nodes and boundaries are written in dependency order, so `flowchart LR` reads from the
entry points to the foundations. The production edges (those backed by at least one import outside
test code) are condensed into strongly connected components; a node's rank is the length of the
longest path in that DAG from a component nothing depends on (rank 0). Nodes are ordered by
`(rank, id)`; a boundary takes the position of its lowest-ranked member, ordered by id among equal
ranks, and its members are ordered by `(rank, id)` inside it. Edges are ordered by the rank and id
of their source, then of their target. A node that only test code connects has rank 0. The edges only test code backs are written as test
edges, `a -->|test| b` (see "Test edges"), and are the ones that do not decide the order.

**Cycles.** After the edges, one comment line per cycle: `%% cycles: a <-> b <-> c` for each strongly
connected component of the production edges with more than one node (members in id order,
components by their first member), then `%% cycles (tests only): ..` for each component of all
edges that is not already one of those, so architectural debt is visible at a glance. A summary
comment `%% N nodes, M edges, K cycles; generated by driftwall init` follows the preamble, K being
the number of cycle lines. All of these are plain comments to the parser.

**Observed packages.** After the cycles, one comment per node that has third-party package uses in
non-test code (see "Package uses"): `%% observed packages web: lit, three`, the names sorted, nodes
in id order, after one comment line that says they are a starting point, not enforced, and that
writing a line as `%% packages <node>: ..` enforces it. The word after
`%%` is `observed`, not `packages`, so the parser reads them as plain comments and the draft is no
stricter than before: only a person or an agent who copies a line into a `%% packages` rule
enforces it. A node with none (and a repository with none) gets no line. The standard library is
not listed.

### Contract and writing

`--map` writes the mapping into the contract and no `.arch` file (`--map` and `--no-tags` conflict).
The nodes, the edges and the owner of every file are those of the run without it, and `lint` on the
result with the same `--contract` reports the same findings: every tag `init` would write, and
every existing `.arch` tag (which would stop being read), becomes map lines, sorted by directory.
A directory tag `node: <id>` at `d` is `%% map <id>: d/`, with `scope: here` it is `d/*`, and at
the root `./` and `*`; a `node: <prefix>*` tag is `%% map <prefix>*: d/`. A `node: *` tag with
`scope: here` has no map form and is a tool error. The contract is checked to round-trip as without
`--map`, existing `.arch` files stay (they are then reported by `ignored-tag-files`), and only the
contract is a conflict without `--force`. The contract is presented as without `--map` (boundaries,
labels, order, cycles), with the `%% map` lines last.

- Every resolved import between files with different owners becomes one solid edge, or a test edge
  `-->|test|` when every import behind it is test-only. Nothing is dotted, so a fresh baseline has no stale or planned-realized findings. Imports are resolved with
  the walked non-source files (assets) known, as `lint` does: an asset (a `.json`, a `.css`) has the
  owner the tags give it, the existing ones and the ones `init` writes, and an import of an asset
  owned by another declared node is an edge. The same holds for an excluded file (see "Excluded
  files"), which imports resolve to too. An asset or excluded file no tag covers adds no edge,
  exactly as in `lint`. **Fresh-baseline invariant:** an asset, or an excluded file that some import
  resolves to, whose owner a tag gives it is owned by a declared node: when the owner is a name only
  such a file derives under a `node: *` tag (`app.css` beside `App.svelte`, `index.css`, a directory
  of fonts, a generated `route-table.ts` beside `bus.ts`), `init` declares it as a node of its own,
  labelled with the directory or path it comes from, so `lint` right after `init` records no
  `undeclared-owner` skipped reference caused by `init`'s own choices. An excluded file that nothing
  imports declares nothing. Every node that a directory tag `init`
  writes names is declared, so an asset in a directory of a unit (`ui/src/index.css`, imported by
  `ui/src/main.tsx`) is owned by a node the contract declares (`ui-src`): `where` and `lint` never
  name a node `init` left out.
- A directory tag that is a group also gets `scope: here`: the repository root group (the node of
  the files directly in the root, usually `node: root`), a unit group and the group of the loose
  files in the source directory of a non-primary JavaScript unit (see "Code units"), provided every
  file the tag owns sits directly in its directory. A directory added later is then
  not covered by inheritance, and `unmapped-file` reports its sources. Every other directory tag
  inherits as always, because the files below it belong to it.
- `init` refuses (exit `2`) when the contract or an `.arch` it would write exists, listing every
  such path, and checks all of them before writing anything. `--force` overwrites. Running `init`
  again therefore needs `--force` only for the contract, and rewrites it to the same contents,
  because every file is then owned by an existing tag.

## Diff beyond the contract

Contract edges are not the only way to make a finding disappear. An agent can move the imported
file into the importing node, or edit a `%% map` glob so a file changes owner, and lint sees a
clean tree. `diff --base <REF>` therefore also reports what changed besides nodes and edges. Every
section below is empty, and then absent from text, when nothing of its kind changed.

**Rules changed.** A `rules changed:` section lists each rule directive line the new contract
renders and the old one does not (`  + %% order: app > core`) and the reverse (`  - ...`). The text
is the line as `render` writes it, so spacing, the order of nodes inside a line and the order of
lines are not changes; a directive whose content changed is a removed line and an added line. The
directives are `%% order`, `%% order open`, `%% isolated`, `%% each`, `%% budget`, `%% tests`,
`%% packages`, `%% forbid-packages`, `%% facade` and `%% no-cycles` (both forms). `%% exports` keeps
its own `+ exports` lines and `%% map` lines are reported through the files they move (below), so
neither is repeated here. The same section lists a node whose boundaries changed, one line each,
`  node billing moved from boundary shared to features`. A boundary is written as the chain that
encloses the node, outermost first, joined by `/` (`app/features`), so a change of nesting moves
every node inside; `(none)` stands for no boundary. JSON: `rules_added` and `rules_removed`, lists
of the lines (trimmed), and `boundary_moves`, a list of `{node, from, to}` with `from` and `to`
strings or `null`.

**Moved between nodes.** A `moved between nodes:` section lists every source file whose owning
node differs between `REF` and the working tree, `  <file>: <old node> -> <new node>` plus a
cause, with `(none)` for no owner (a file that became unmapped, or newly mapped):

- `(mapping changed)`: the path exists on both sides and its owner changed because the mapping
  did: `%% map` lines, module-form maps or `.arch` tag files.
- `(renamed from <old path>)`: git's rename detection (`-M`, between `REF` and the working tree
  including untracked files) pairs the file with a path at `REF`, and the owner of the new path
  differs from the owner of the old.
- `(renamed from <old path>, mapping changed)`: both, which is when the new path would have had
  a different owner under the mapping at `REF` as well.

A file added or deleted outright is not a move and is not listed. Only source files are
compared (assets, excluded and ignored files are not), and a folded entry file has no owner.

The owner at `REF` is computed exactly like lint computes it: the tree at `REF` is exported to a
temporary directory and scanned with the contract found there (`%% map` lines when it has any,
`.arch` files otherwise, a missing contract counting as empty), with the same walk, ignores and
module rules as the working tree. An `.arch` file or contract at `REF` that cannot be read or
parsed is a tool error, exit `2`. Rename detection adds the working tree to a temporary git index whose objects go to a temporary
directory, with the repository's own objects as an alternate, so `diff` writes nothing into the
repository.

Text and JSON are sorted by file. JSON: `moved`, a list of `{file, from_node, to_node,
renamed_from, mapping_changed, cause}`; `from_node` and `to_node` are `null` for no owner,
`renamed_from` is absent unless renamed, and `mapping_changed` is a boolean. `cause` names the
combination of the two: `mapping-changed`, `renamed` or `renamed-and-mapping-changed`.

**Exit code.** `--exit-code` already fails on any reported change; a rule change, a boundary move
and a moved file are reported changes, so each fails it. There is no flag to allow a move: the
change is reported and the human approves it in review.

## Exit codes

`0` clean. `1` violations. `2` tool error (bad diagram syntax, IO failure, bad arguments).
A failing lint is not an error, so `anyhow` is used only at the entry point and always maps to `2`.

## CLI surface

```
driftwall lint [PATH] [--contract FILE] [--format text|json|sarif] [--strict] [--baseline FILE | --write-baseline FILE]
driftwall init [PATH] [--depth N] [--contract FILE] [--no-tags | --map] [--force]
driftwall where <FILE> [--root DIR] [--contract FILE] [--format text|json]
driftwall brief [PATH] [--contract FILE] [--format text|json]
driftwall neighbors <NODE> [PATH] [--contract FILE] [--format text|json] [--files] [--all-sites]
driftwall diff --base <REF> [PATH] [--contract FILE] [--format text|json] [--exit-code] [--baseline FILE]
```

`--format json` is the agent interface. Every violation carries `kind`, `severity`, `at`
(`file`, `line`), `message`, `hint`. A violation with a baseline key also carries `key`, `<kind> <key>`, see "Grouped findings".

A finding at an import site written in test code (`test_only`, see "Import kind") says so: `undeclared-edge`,
`private-symbol` and `upward-edge` carry the mark; no other kind does. The text puts `(test code)` after the kind, `src/a.test.ts:3: error[undeclared-edge]
(test code): ...`, so `file:line:` stays the start of the line. The JSON violation has `"test_only": true`,
the key being absent on any other finding, so existing consumers see no change. A SARIF result has
`"properties": {"testOnly": true}`, also only then. Whether a test site is allowed is decided by the edge
kinds as before; the mark only tells the reader that the site is test code.

### Grouped findings (text output)

One bad edge used at twelve import sites is one problem, so the `lint` text output groups findings
that share a key. JSON and SARIF do not: they stay one result per site, the stable interface for
agents and for code scanning, which needs a result per line.

- **The grouping key** is the kind plus `Violation.key` of "Debt baseline" ("Keys"), so a test-only
  site (key ending ` (test)`) is its own group. A finding with no key never groups, and neither does
  a finding without a location. A group of one prints exactly as before, byte for byte.
- **A group of N > 1** prints a header, one line per site, and the hint once:

  ```
  src/billing/a.rs:3: error[undeclared-edge]: `billing` imports `search` in 3 places but architecture.mmd declares no edge billing --> search
    at src/billing/a.rs:3 (src/search/mod.rs)
    at src/billing/b.rs:8 (src/search/mod.rs)
    at src/billing/c.rs:1 (src/search/index.rs)
    hint: remove the import, or ask the user to approve adding the edge to the contract
  ```

  The header starts with the first site's `file:line:` so editors still jump. The site lines are
  sorted by file, then line, and say `(<target file>)`, plus `, <symbol>` for a `private-symbol`
  site. A site whose finding has no target (a package use, an unresolved import) has no
  parenthetical. The `(test code)` mark of "Import kind" stays after the kind in the header: every
  site of a group shares it.
- **The header message** is the first site's message with its site-specific part, the target file,
  replaced by `in N places`: `` `a` imports `b` (src/b/x.rs) but ... `` becomes
  `` `a` imports `b` in N places but ... ``. Where the target sits elsewhere the sentence is kept
  whole: `upward-edge` and `isolation-breach` write `(boundary `u`) in N places`,
  `production-imports-test` writes `` imports test file `t` in N places ``, and `forbidden-package`
  (which has no target) writes `` imports `pkg` in N places ``. A kind or message the generaliser
  does not recognise ends in ` (N places)` instead, so a group always reads as a group. Kinds that
  are one per key (`stale-edge`, `unmapped-file`, ...) never have two members, so never group.
- **The baseline suffix** `; the baseline allows <N>, found <M>` appears once, at the end of the
  header, not on every site.
- **Order:** a group is printed where its first site would have been, in the order findings have
  today (kind, then location); the sites inside it are sorted by file and line.
- **The summary line** still counts findings, not groups. When at least one group has more than one
  site it gains ` (K groups)` after the warning count and before `, N baselined`: `driftwall: 12
  errors, 0 warnings (3 groups)`. K counts the printed blocks, a single finding being a group of
  one. Without grouping the line is unchanged.
- **JSON** violations gain `"key": "<kind> <key>"`, the grouping key, so a consumer can group the
  same way; it is absent for a finding with no key (`stale-baseline`). It is additive: nothing else
  in the JSON or SARIF changes.

`where <FILE>` says what a path is and who owns it. The path must exist (inside `--root`, default
`.`): a missing one is a tool error, exit `2`, `no such file: <path>`. `--contract` is accepted like
everywhere else and must name a readable, valid contract when given; `where` reads nothing else from
it, and scans the repository only for the package uses of a source file (below). The text is `<file>: <answer>`, one of

| What the file is | Answer | Exit |
| --- | --- | --- |
| source file with an owner | `<node> (via <arch file>)`, with a map `<node> (via <contract>:<line> (map <glob>))` | `0` |
| source file no tag covers | `unmapped` | `1` |
| folded entry file (see "Module nodes") | `folded, no node (via <arch file>)` | `0` |
| asset | `asset of node <node> (via <arch file>)`, or `asset, unmapped` | `0` |
| unread code file (see "Blind spots") | `not read (<key>) in node <node> (via <arch file>)`, or `not read (<key>), unmapped` | `0` |
| excluded source file | `excluded (<reason>), owned by <node> (via <arch file>)`, or `excluded (<reason>)` when no tag covers it | `0` |
| source file `.driftwallignore` hides | `ignored by .driftwallignore, owned by <node> (via <arch file>)`, or `ignored by .driftwallignore` when no tag covers it | `0` |
| an `.arch` file | `tag file` | `0` |
| the contract (`--contract`, default `architecture.mmd`) | `contract` | `0` |

An excluded or ignored file is owned by the tag rules of source files, as `brief` and `lint` attribute it
(see "Excluded files" and "Blind spots"), so `where` and they name the same node. The `.arch` files and
the contract are driftwall's own input: they are never assets, and the answer names no node. A directory
is answered like a source file, by the tag that covers it. The JSON has `file`, `node`,
`arch_file` and `kind` (`source`, `folded`, `asset`, `unread`, `excluded`, `ignored`, `tag-file`,
`contract` or `directory`), plus `detail` for the unread key and the exclusion reason (absent
otherwise). `node` and `arch_file` are `null` for anything no tag covers, a tag file and the contract,
and carry the owner of an excluded or ignored file that a tag covers. `owned_by` says which
mechanism gave the owner: `tag` (an `.arch` file) or `map` (a `%% map` glob). It is absent when
the file has no owner, also for a folded entry file, so an agent does not infer it from which keys
are present.

For a source file that uses packages (see "Package uses") `where` also lists them, in line order,
after the answer, one per line: `  package <name> (<kind>) line <n>`, followed by ` (test)` for a use
in test code. The JSON gains `packages`, a list of `{line, name, kind, test_only}`, absent when the
file uses none and for every other kind of file. The list needs a scan of the whole repository, so it
needs a valid contract: with none, `where` answers as before and lists nothing.

`brief`, `neighbors` and `diff` are read-only maps for agents and reviewers:

- `brief` prints every node (directories, file count, see "Node directories" below; a node with no
  source file that owns assets or excluded files prints `0 source files, N assets` or `0 source files,
  N excluded` (both, in that order, when it owns both) in place of `0 files`; every node's JSON has
  `assets`, the number of asset files it owns, and `excluded`, the number of excluded files it owns:
  excluded files are not assets, and neither is in `files`) and every edge, declared or observed, with
  the number of import sites that realise it and how they are used (kinds, symbols, see "Import
  kind" and "Symbols and public surfaces"). An edge code uses and the contract lacks is marked
  `UNDECLARED`, and one `lint` has a finding for `STALE`, `PLANNED-REALIZED` or `TEST-ONLY`, see
  "Edge status". It is informational: exit `0` unless the tool fails (`2`).
  It ends with the blind spots block, and a node line ends with ` blind: 3 not read, 1 excluded,
  2 ignored, 4 skipped, 1 unresolved` (only the non-zero parts; absent when all are zero):

  ```
  blind spots:
    not read: swift 40 files (1200 lines), sh 10 files (300 lines)
    excluded: vendored 3, generated 2
    ignored by .driftwallignore: 4
    skipped references: undeclared-package 5, dynamic-specifier 1
      undeclared-package:
        a/main.ts:2 left-pad
      dynamic-specifier:
        a/main.ts:4 modName
    unresolved imports: 2
    unmapped: 2
      not read: loose/tool.pl (pl, 1 line)
      excluded: loose/app.min.js (minified)
  ```

  A line is present only when its count is not zero (`unmapped` counts the items attributed to no
  node, over all categories). The `unmapped` line is followed by those items, one per line and four
  spaces indented, in the forms and the order of the `blind spots:` block of `neighbors` below (not
  read, excluded, ignored, skipped, unresolved), so every one is named by its path. At most 20 are
  listed, then one line `    +N more` for the rest. The `skipped references` line is followed by the
  references themselves, grouped by reason (in the fixed order of the "Skipped references" table)
  with the reason as a header line, each as `file:line specifier` (and ` - <message>` when it has
  one), so `brief` lists what the `note:` line points to. The cap is per reason: every reason that
  occurs is listed, with at least its first 5 references (sorted by file and line), then one line
  `    +N more, see --format json` under that reason when it has more. A rare reason is therefore never
  hidden behind a common one. Unread extensions are ordered by file count, then name. Excluded
  reasons and skip reasons are in the order of this document. When every count is zero the block is
  the single line `blind spots: none`. In JSON, `blind_spots` has `unread` (key to `{count, lines}`),
  `excluded` (reason to count), `ignored`, `skipped` (reason to count), `unresolved`, `unmapped`
  (a tally with the same five counts) and `unmapped_items` (every item no node owns, uncapped, in the
  shape of the `blind_spots` of `neighbors` JSON: `unread` per key with `count`, `lines` and `files`,
  `excluded` per reason, `ignored`, `skipped` per reason, `unresolved`, each item with its file and
  `node: null`), `skipped_references` (every skipped reference, uncapped, by reason, each with
  `file`, `line`, `specifier`, `node` and `message` when it has one), and every node has `blind_spots`, a tally with `unread`, `excluded`,
  `ignored`, `skipped` and `unresolved` file or reference counts.
- **Node directories.** The `dirs` of a node, in `brief`, `neighbors` and the JSON `dirs`, are where
  its code lives, not where its `.arch` file lies. The name is kept for compatibility: an entry is a
  repository path, a directory or, for a one-file module, the file itself (`src/fonts.ts`). They are
  the union, sorted and without duplicates, of

  - the directory of every `.arch` file whose plain `node: <id>` names it (the repository root is
    `.`), also when no file is left under it;
  - the module path of every source file the node owns that a `node: *` or `node: <prefix>*` tag
    owns: the tagged directory joined with the first path segment below it. A file in a
    subdirectory is the directory module `src/ui/**`, shown as `src/ui`; a file directly in the
    tagged directory is a file module, shown as the file itself (`src/gitlab.rs`; `a.ts` and
    `a.test.ts`, which share the node `a`, are two entries). The tagged directory itself is never
    shown for such a node.

  A node with no entry of either kind, because it owns no source file (`app` owning only
  `src/app.css`), shows instead the module paths, by the same rule, of the asset files and excluded
  files it owns (`src/app.css`), so it never reads as empty.

  A `scope: here` group is a plain tag: its directory. The rule reads only the source files the
  node owns and the tags, so it is the same in `brief`, `neighbors` and JSON, and for every run.
- A contract with rule directives (see "Contract rules") adds to `brief`: a `rules:` block after the
  edges, one line per directive (`  order: entry > engine > shared`, one line per `%% order` chain, `  no-cycles`,
  `  no-cycles: boundaries`, `  tests: isolated`, `  isolated: features (billing, search)`, `  facade: ui-lib, wire`), and for a node in a listed
  boundary ` layer: engine (2 of 3)` (the place in the first chain that lists a boundary around the node) and for a node with a budget ` budget: fan-out 3/5, files 21/20
  over` at the end of its line, before the blind spots. In JSON the
  document has `rules` (`order`, a list of chains, each the boundaries top first; `no_cycles` and `no_cycles_boundaries`,
  `true` when the line is there; `tests_isolated`; `isolated`, a list of `boundary` and `children`; `facades`, the node ids sorted) and the node has `layer` (`boundary`,
  `rank` from 0 at the top, `of`); both are absent when the contract has no such rule, so a contract
  without rule directives reads as before. `neighbors` prints the same `layer:` and `budget:` lines after `dirs:`.
- **Packages.** A node that uses packages lists them in `brief` and `neighbors`, third party
  packages first, then the standard library, each group by name. `brief` ends the node line with
  ` packages: lit (third-party, 4), node:fs (stdlib, 2)` (the first six, then `+N more`, before a
  ` blind:` part); `neighbors` prints a `packages:` section, one `<name> (<kind>, <uses>)` per line
  after `excluded:` and before `outgoing:`. The number is the uses in the node's files, and
  `, <n> test` follows it when `n` of them are in test code (`vitest (third-party, 3, 3 test)`). In
  JSON every node (and `neighbors`' `node`) has `packages`, a list of
  `{kind, name, count, tests}` in the same order, absent for a node that uses none, so a repository
  that imports nothing outside itself prints as before.
- `neighbors <NODE>` prints one node's incoming and outgoing edges with up to 3 `file:line`
  example sites each (`--all-sites` prints every site; JSON always has every site, with no cap
  anywhere). `--files` lists the node's source files under its `files:` line, two spaces indented
  (JSON always has `files`, the sorted list). A node that owns assets has an `assets: <n>` line
  after them and one that owns excluded files an `excluded: <n>` line after that, and `--files` lists
  each the same way (JSON `asset_files` and `excluded_files`). It also lists, always, everything the node is blind to
  in a `blind spots:` block, one item per line (`blind spots: none` when it has none):
  `  not read: <file> (<key>, <n> lines)`, `  excluded: <file> (<reason>)`,
  `  ignored: <file>`, `  skipped: <file>:<line> <specifier> (<reason>)` (followed by ` - <message>`
  when the skip has one) and `  unresolved: <file>:<line> <specifier>`. JSON has the same items under
  `blind_spots` (`unread` per key with `count`, `lines` and `files`, `excluded` per reason, `ignored`,
  `skipped` per reason, `unresolved`; every item has its `node`, and `lines` for unread files). An
  unknown node is a tool error that lists the declared ids.
- `diff --base <REF>` compares the contract at a git ref with the working tree (nodes and edge
  kinds, boundary edges included, see "Boundary edges"; labels and boundaries are ignored). The old text comes from `git show REF:./<contract>`;
  a contract absent at that ref counts as empty, any other git failure is exit `2`. Exit `0`
  normally; with `--exit-code`, `1` when anything changed, so CI can require approval.
  With `--baseline <FILE>` it also reports debt added to the baseline, see "Debt baseline".
  It also reports rule directive changes, boundary moves and files that changed owner, see "Diff
  beyond the contract".

`lint` text output ends with the note line of "Blind spots" when any blind spot exists, after the
summary line, and its JSON gains a `blind_spots` object (the summary of `brief`, without `unmapped`)
as its last key. Exit codes are those the findings decide, whatever the blind spots; SARIF is
unchanged. `init` prints the same note line after the line that says what it wrote.

## Module map

The contract for this repository is `architecture.mmd`, enforced on itself once Rust support
lands. `src/<node>/.arch` tags each module. `model` has no outbound edges. `report` only renders
`model` types. `lint` and `init` are the only modules that combine `diagram`, `ownership` and
`imports`.

## Enforcement over instructions

Anything a tool can check is enforced by a tool and kept out of prose (`AGENTS.md`, the skill).
Style and error handling are clippy lints, unordered collections are `disallowed-types`, missing
docs are `missing_docs`, undeclared dependencies are `driftwall lint`. The agent skill carries
only judgment: when to discuss with the user, when to stop and ask. Everything else is a hook or a
command, for example a hook that runs `driftwall lint --format json` after edits.

The repo applies this to itself. `scripts/gate.sh` is the pre-commit hook, `scripts/hooks/guard.py`
blocks agent actions that weaken the gates (pkill, `--no-verify`, pushes, edits to the contract and
lint configuration from a worktree), and `tests/style.rs` requires every plain comment to start
with `why:`. The golden rule is to leave an agent as little room to err as possible.

## Quality bar

- Lints live in `Cargo.toml` `[lints]`. CI runs `cargo clippy --all-targets -- -D warnings`.
- No `unwrap`, `expect`, `panic`, indexing or slicing outside tests. `unsafe_code` is forbidden.
- `thiserror` in the library, `anyhow` only in `src/cli`. Only `src/cli` prints.
- Every collection that reaches output is ordered. Output is byte-identical across runs.
- Real strictness is tests: fixture repos per language, `insta` snapshots for text and JSON
  output, and driftwall linting itself.

## Milestones

1. M1 ownership: walk, `.arch`, ignores, `where`. No parsing.
2. M2 diagram: parser and renderer for the subset above.
3. M3 imports (TS/JS): extraction, relative and index resolution.
4. M4 lint: reconcile, `report`, JSON output, exit codes. Usable end to end here.
5. M5 imports (Python).
6. M6 init: baseline with `--depth` and `--write-tags`.
7. M7 imports (Rust), then driftwall lints itself in CI and the dotted edges turn solid.
8. M8 agent integration: Claude Code skill and hook, `neighbors`, `diff`.
9. M9 release: cargo-dist builds, npm package, GitHub release.
10. M10 designed contracts: `%% map` lines keep the code-to-node mapping in the contract, `init`
    drafts read as a design (boundaries, labels from the code, layer order, cycles), and the
    dogfood projects carry a hand-designed contract whose violations are their real debt.
11. M11 component rules, each one `%%` line per node, checked from the AST like edges:
    - packages: which third-party packages a node may import (only `db` imports `sqlx`), and
      which packages, the standard library included, it may not (done: `%% packages`,
      `%% forbid-packages`, see "Package rules");
    - type-only edges: a node may use another's types but not its runtime code (done: `-->|type|`,
      see "Type edges");
    - test-only edges: production code may not import test helpers (done: `-->|test|`, see
      "Test edges", and per helper with `%% tests: isolated`, see "Test isolation");
    - layer order: a node may use only nodes in lower boundaries, so arrows need not be drawn
      (done: `%% order:`, see "Layer order");
    - no cycles between named nodes or boundaries (done: `%% no-cycles`, see "No cycles");
    - budgets: maximum fan-out or size per node (done: `%% budget`, see "Budgets").
