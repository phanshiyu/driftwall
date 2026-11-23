# driftwall dogfood review rubric

You review how well driftwall models ONE real project. driftwall parses source files with
tree-sitter, resolves imports (and Rust qualified paths) to files, maps files to nodes (`.arch`
tags; `node: *` makes each module in a directory its own node), and compares that graph with the
contract `docs/architecture.mmd`. `driftwall init` generated the contract and tags from the code.

Inputs: the project worktree `<out>/<name>/` made by `scripts/dogfood.sh` (contract in `docs/architecture.mmd`, tags in
`.arch` files), the evidence folder `<out>/<name>.evidence/` (init, lint, brief text and JSON,
timing, extension census), and the pinned binary named in your brief. You may run the binary
(`brief`, `neighbors <node>`, `where <file>`, `lint --format json`, all with
`--contract docs/architecture.mmd`). Do NOT edit any file in the worktree or anywhere else. Do not
commit or push.

Judge each criterion with evidence you checked yourself in the code (file:line), never on
driftwall's word:

1. Runs: init and lint exit 0, no tool errors, under 30 seconds.
2. Granularity: the nodes reflect the project's real architectural units. A node holding most of
   the codebase, or a pile of trivial one-file nodes for config files, is a failure. Say what the
   right units would be.
3. Edge precision: pick at least 10 reported edges (or all if fewer) and verify each against its
   cited import sites. Any edge that does not exist in code is a defect.
4. Edge recall: pick at least 10 source files spread across nodes; list every import in them that
   targets another repository file in a different node; check each appears as an edge. A miss is
   a defect unless it falls under a documented limitation (method calls on values, unsupported
   languages), and even then it must be visible in driftwall's output.
5. Symbols: on at least 5 edges, check the recorded symbols (`brief --format json` or
   `neighbors`) match what the code imports.
6. No silent blind spots: files in languages driftwall cannot read (e.g. .svelte, .swift, .vue,
   .sh), ignored or unresolved imports, and skipped configs must be visible to a user of
   driftwall's output. Silent omission of a meaningful share of the code is a failure.
7. Usefulness: would a coding agent given `brief` and `neighbors` output understand this project's
   structure and its dependency rules? Name what is missing or misleading.

Classify every finding as either a DRIFTWALL DEFECT (the tool is wrong, incomplete, or misleading;
give a minimal reproduction and the expected behavior) or a PROJECT OBSERVATION (a true fact about
the project's architecture worth reporting to its owner, e.g. a dependency cycle).

Verdict per project: SATISFACTORY only if criteria 1 to 7 all pass. Otherwise UNSATISFACTORY with
the blocking defects listed first, most severe first.

Output for each project, in this order: verdict; blocking defects (with repro); non-blocking
defects; project observations; the edges and files you checked (so the next round can re-check).
