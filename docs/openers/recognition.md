# Opener Recognizer Reference

## Purpose And Scope

This is the maintainer reference for FUSION's current symbolic opener recognizer: the production path from a runtime catalog to a serialized round result, plus the test-only evidence machinery that keeps compiler and alignment contracts honest.

The recognizer analyzes **observed rounds**. It does not rank speculative
search nodes and does not alter beam search, versus simulation, sequence
simulation, or training labels.

The catalog is installed at runtime, not embedded in the WASM binary. Browser-side code supplies catalog JSON through the catalog-install seam before asking FUSION to analyze a round.

## Compact Flow

```text
catalog JSON (+ optional witness companion JSON)
  -> install_opener_runtime
  -> validated InstalledCatalog snapshot in the runtime install slot
  -> analyze_opener_round
   -> prepare observations once, retaining slot ordinals and garbage evidence
   -> weak per-lock possible-build assessment against installed targets
   -> whole-round catalogued-board confirmation + singleton projection
   -> ordered, unique route-recognition candidate set
  -> compile each eligible record and its mirror into a recognition graph
  -> exact-index seeding + min-plus alignment of observed locks
  -> per-lock evidence and ranked record hypotheses
  -> opener guide subject (confirmed or nearest) and guide card
  -> OpenerRoundAnalysis serialized by the WASM seam
```

## Runtime Installation And Entry Points

### Install Contract

`install_opener_runtime(catalog_bytes, witness_bytes?)` parses the catalog as
JSON, requires `formatVersion == 2`, runs catalog validation, optionally parses
and validates the witness companion against that catalog, and only then
replaces the process-local install slot with one `InstalledCatalog` snapshot.
The snapshot owns derived targets, node boards and accepted runtime witness
targets. Its `RecordGraphCache` owns the immutable catalog and lazy record graphs;
`catalog()` and `Deref` expose read access to that same catalog. A rejected
companion leaves the previously installed runtime untouched. It returns
`{catalog: CatalogStats, witnesses: WitnessCatalogStats | null}`, not a
compiled graph. See [`catalog.rs`](../../src/openers/catalog.rs) and
[`install_opener_runtime`](../../src/openers/catalog.rs).

Rust callers, tests and the WASM seam share this combined installer.

Catalog records contain tree nodes with parent/piece/row data, optional
pre-clear/clear rows and placements, a grey flag, and an optional route name;
these authored frames are compiler input, not a generated cache. See
[`OpenerCatalog` and tree types](../../src/openers/catalog.rs).

The public WASM methods are:

| Method | Input | Success result | Failure result |
| --- | --- | --- | --- |
| `install_opener_runtime` | catalog JSON bytes, optional witness JSON bytes | `{status:"ok",data:{catalog,witnesses}}` | malformed, unsupported-version, or invalid-catalog error; malformed, unsupported-version, identity-mismatch, or invalid witness error |
| `analyze_opener_round` | `OpenerRoundInput` JSON bytes | UTF-8 bytes of `{status:"ok",data:OpenerRoundAnalysis}` | UTF-8 bytes of an `invalidInput` or `noCatalog` error |

`analyze_opener_round` is bytes-in/bytes-out: the caller encodes the input
JSON once and decodes the returned JSON once, so no `JsValue` round trip or
intermediate `JSON.stringify`/`JSON.parse` happens on the engine side.

The envelope, error mapping, and bindgen exports are in
[`wasm.rs`](../../src/wasm.rs).
Analysis before installation returns `AnalyzeError::NoCatalog`, not a built-in asset.

### Witness Companion Contract

The witness companion is a versioned positive-receipt asset. Installation
requires its catalog hash to equal the exact catalog bytes installed alongside
it, and validates each witness against the catalog record and search-shape
occupancy. A runtime target is accepted only when its frame is
`postClear`, its scope is `freshRound`, it carries a nonzero absolute ordinal,
and its normalized occupancy equals the catalog search shape. Installing a new
catalog without a companion clears previous companion targets. The private provenance/enrichment
report is not consumed by Fusion and is not published.

### Round Input And Orchestration

`OpenerRoundInput` carries two fields: `observations`, the ordered per-lock slot
array, and optional `relaxDepth`. Each slot is nullable: a null slot marks a lock the
caller recorded no observation for. The key set is strict: unknown fields, including
the retired `dealtInputs` and `tailQueue`, produce `invalidInput` at the WASM seam.
An omitted `observations` key is an empty round rather than an error. Callers must
use this reduced request shape when loading the rebuilt package; see
[`analyze.rs`](../../src/openers/analyze.rs).

The orchestrator prepares observations once through `prepare_observations` in
[`phase.rs`](../../src/openers/phase.rs). This internal view retains every slot,
distinguishes an absent slot from a present observation missing its board or mask,
and preserves original garbage presence alongside the garbage-stripped board and
aligned optional letters. Preparation preserves that observation shape unchanged;
the reduced request shape above is the change callers must adopt.

Assessment, catalogued-board confirmation, route recognition and the guide reuse
that prepared view. Their policies remain separate; the test-only detached-catalog
path shares the same orchestration. See [`analyze.rs`](../../src/openers/analyze.rs).

`relaxDepth` affects only attack-gap policy: the lock must be on script and its index less than `relaxDepth` when provided. B2B-break and board-mess exemptions follow `onScript`; tier-distribution inclusion is the inverse. See [`lock_policy`](../../src/openers/analyze.rs).

## Opener Guide

The guide is the reference card the client shows for the opener a round built
or came nearest to. It carries the catalogued target shape per phase (root to
the matched node, in the player's chirality), the record's documented
requirements (`dependencies` text verbatim, mirror clause promoted when the
player built the mirror; documented first-bag `cover.pct`; `pcChance`; tags),
the follow-up variations (children of the matched node, or its siblings when
it is a leaf; capped at `GUIDE_VARIATION_LIMIT` with the total reported), the
record's source links, and a deviation. It never shows or implies a placement
order: the catalog carries none, and an engine-invented order mis-teaches.

`basis` says how the subject was chosen:

| Basis | Condition |
| --- | --- |
| `confirmed` | Confirmation names one record; or confirmation ties on identical boards and recognition's top hypothesis is one of the tied records with `margin > 0`. Recognition breaks the naming tie only; on-script policy is unchanged. |
| `nearest` | Confirmation is silent; the top hypothesis has `totalCost < unknownCost`; `margin > 0`, or `margin == 0` and every tied record shares the top record's `shapeKey`; and the hypothesis is not `identityFrozen`. |

Otherwise there is no guide. On the 34-round perf fixture this yields 25
confirmed (4 of them tie-broken by recognition), 2 nearest, and 7 silent
rounds; every silent round is a margin-0 tie across unrelated records.

The deviation compares the player's board with the shape they were building
toward at that shape's locked-piece ordinal, so both boards hold the same
number of placed pieces. `divergenceLock` is the first lock past the confirmed
anchor whose alignment paid a non-zero cost; a divergence past
`OPENER_PHASE_LOCKS` is ordinary play and yields no deviation. For a confirmed
opener the compared shape is the anchor's continuation that best overlaps the
player's board (letters agreeing count double); a finished opener with no
catalogued continuation has no deviation. For a nearest opener it is the phase
in progress at the divergence. Counts are `missing` (target cell, player
empty), `stray` (player cell, target empty), and `wrongLetter` (both
occupied, neither a wildcard, letters differ); the client classifies cells the
same way from the two boards.

See [`guide.rs`](../../src/openers/guide.rs).

## Possible-Build Assessment And Confirmation

### Assessment Defaults

| Contract | Live value | Effect |
| --- | ---: | --- |
| `OPENER_PHASE_LOCKS` | 14 | Canonical opener window; not caller-configurable |
| phase match limit | 8 | Number of board matches requested per assessed lock |
| grey minimum progress | 0.85 | Grey target needs this progress unless colored or complete |
| retained runners-up | 2 | Candidates retained after selecting the best match |

The constants, runner-up limit, and predicate are in
[`phase.rs`](../../src/openers/phase.rs). Assessment uses the garbage-stripped board;
an empty board is on script. Otherwise the placed-cell deficit must be
nonnegative and divisible by 10, and a qualifying match has no stray cells and
is colored, complete, or meets 0.85 grey progress. After index 13 it continues
only when the previous assessment is on script. Missing observation/board/mask
data produces no assessment.

### Catalogued-Board Confirmation

Confirmation is independent of possible-build assessment. Fusion compares occupied
cells only at the exact absolute 1-based locked-piece ordinal carried by a
catalogued tree node. Piece letters, documented build order, parent traversal,
and continuous route evidence do not gate achievement.

`match_catalogued_boards` scans every observation slot and retains null slot
ordinals. The first board match initializes every matching record. Only later,
deeper boards in that survivor set can narrow it. A miss never revokes the result;
a later exact board can rejoin. Unrelated later records never join, and authored
post-PC descendants stay in the same result. See
[`catalogued_match.rs`](../../src/openers/catalogued_match.rs).

`RoundCataloguedBoardMatch` carries `firstMatchIndex`, `anchorIndex`, and the full
deterministically ordered `matchingOpeners` set. Each match carries ID/name,
deepest pieces, candidate node IDs, nullable unambiguous route, and nullable
orientation. `report` is only a nullable singleton presentation projection: it is
absent for a tie and carries no verdict, progress, on-script length, or PC anchor.

Current `searchShapes` remain parsed/countable for asset compatibility. Positive
offline enrichment receipts arrive through the separate witness companion; only
its independently validated post-clear fresh-round targets participate in the
additive catalogued-board matcher. The existing tree and possible-build phase
targets are not rewritten. Runtime must not infer source intent from earliest
reachability.

## Shortlist And Compilation

### Shortlist Contract

Route recognition is independent from confirmation and scans all observations. Its
candidate selection does not use a report primary or a first-PC window. When a
catalogued-board match exists, every surviving record enters the candidate set with
no 24-record cap and shortlist truncation is false. Without confirmation, weak
assessment matches and runners-up retain the historical 24-record cap. Recognition
returns `None` for no usable board or no eligible compiled record. See
[`recognize_round`](../../src/openers/recognition/round.rs).

Each snapshot lazily compiles shortlisted records independently and caches both
successes and compile failures. Missing IDs are not cached. A record that fails
compilation or reaches a compile budget is counted in `shortlistCompileSkipped`
and excluded; production never exposes a budget-truncated record.

A singleton reuses its immutable graph after the total-budget check. Multiple
parts first use a bounded graph union. If the union cannot fit, `merged_compile`
applies the retained eligibility/order contract; a rejected merged result returns
`None`, not partial success. See [`RecordGraphCache`](../../src/openers/recognition/cache.rs).

`CompileBudgetState` owns the production budget verdict independently of metric
callbacks. `ProductionMetrics` records nothing; test-only census, battery and
collision metrics retain the budget evidence. Direct-impossible and
frame-inconsistent bridges can still reach production alignment as degraded
evidence.

See [`compile_records`, `compile_borrowed_records`, `CompileBudgetState` and
`CompileMetrics`](../../src/openers/recognition/compile.rs).

### Current Compile Defaults

| `CompileBudget` field | Default | What it bounds |
| --- | ---: | --- |
| `max_states_per_edge` | 16,384 | States generated for one tree edge |
| `max_placements_per_edge` | 32 | Placements considered for one tree edge |
| `max_dfs_visits_per_edge` | 20,000 | Sequential-DFS visits for one tree edge |
| `max_total_states` | 4,000,000 | Total graph state count |

These production defaults are in
[`CompileBudget`](../../src/openers/recognition/compile.rs). The compiler
skips stub/empty records and compiles both orientations; the census version also
collects metrics and validates epsilon acyclicity. See
[`compile paths`](../../src/openers/recognition/compile.rs).

### Declared Frames And Physical Shadow

Catalog frames use declared occupancy: full rows can persist in an authored
child frame until that edge's declared clear. The engine must still test SRS
legality on a physical board where full rows clear immediately.

For every placement, `DeclaredFrame` removes the placement from the child
pre-clear frame, verifies that this start attaches to its parent, translates the
placement down by the count of declared full rows beneath it, and then validates
the final declared frame, declared `clearRows`, and physical post-clear board.

See [`DeclaredFrame`](../../src/openers/recognition/frames.rs),
[`physical_placement`](../../src/openers/recognition/frames.rs), and
[`endpoint validation`](../../src/openers/recognition/frames.rs).

Placements come from authored node placements when present; otherwise they are
derived from parent rows to the child pre-clear frame. The compiler supports both
original and mirror piece/cell geometry.

See [`placements_for`](../../src/openers/recognition/record.rs).

### Graph Structures And Transition Labels

`RecognitionGraph` contains model states, outgoing transitions, and an
occupancy-only exact index. `ModelState` has a physical canonical key, all
compatible origins, and an `identity_opaque` flag. `StateOrigin` records the
record id, orientation, node id, placed subset, optional route name, and
bag-completion status.

See [`graph data structures`](../../src/openers/recognition/graph.rs).

Canonical keys fold mirrors. Their occupancy form removes letters for exact
lookup; letter information remains available to edit scoring. Thus alignment
cost is chirality-invariant, while known letter disagreements can still cost
edits.

See [`CanonicalKey`](../../src/openers/recognition/graph.rs).

Transition labels are:

| Label | Normal source | Meaning |
| --- | --- | --- |
| `Lock` | Legal physical placement | Consume a model lock |
| `Epsilon(BagBoundary)` | Grey terminal or placement-free edge | Change control/origin without consuming an observation |
| `Bridge(DirectImpossible)` | Unbuildable edge | Explicit degraded connection |
| `Bridge(BudgetExceeded)` | Per-edge budget exceeded | Explicit degraded connection |
| `Bridge(FrameInconsistent)` | Authored frame contradiction | Explicit degraded connection |

The label enum is in [`graph.rs`](../../src/openers/recognition/graph.rs).
The reason payloads are compiled for tests, while the transition kinds and their
runtime behavior are always present.

Grey terminals use the immediate-clear physical shadow and are identity opaque.
Entering them preserves alignment evidence but freezes record identity claims.
See [`physical_shadow_of_declared`](../../src/openers/recognition/graph.rs)
and [`compile_grey_terminal`](../../src/openers/recognition/edge.rs).

Bridges create a physical terminal from the declared child frame, retain its
record origin, and mark the state as bridged. They deliberately do **not** set
`identity_opaque`; a bridge's observed transition instead receives the
identity-opaque-entry edit charge. This retains attribution for v1 evidence,
but a bridged edge is degraded compilation evidence, not normal compiled
success.

See [`compile_bridge`](../../src/openers/recognition/edge.rs) and
[`bridge scoring`](../../src/openers/recognition/align/frontier.rs).

## Exact Alignment And Attribution

### Current Alignment Defaults

| Setting | Default | Role |
| --- | ---: | --- |
| `SeedBudget.max_seeds` | 8,192 | Marks over-large exact seed sets/reseed work |
| `AlignBudget.max_active_product_states` | 4,096 | Nominal positive-cost frontier cap per observed lock; all zero-cost states are retained beyond it and the excess is disclosed |
| final regular hypotheses | 8 | Best per-record route hypotheses retained |

`SeedBudget` is defined in
[`retrieval.rs`](../../src/openers/recognition/retrieval.rs), and
`AlignBudget` in [`align.rs`](../../src/openers/recognition/align.rs).
The final-hypothesis rule is in
[`align/evidence.rs`](../../src/openers/recognition/align/evidence.rs).

Exact seed ties are deliberately not discarded, even when they exceed 8,192;
the run is marked truncated. Seed ordering charges an opaque-entry cost before
breaking ties by state id. This protects equal canonical collision identities
from arbitrary loss while disclosing bounded work.

See [`seed_candidates`](../../src/openers/recognition/retrieval.rs) and
[`ordered_exact_seeds`](../../src/openers/recognition/align.rs).

For each present observation, the aligner increments the unknown lane, exact
seeds the graph, explores synchronous lock/bridge edits plus observed-only and
model-only alternatives, closes epsilon transitions, then truncates the active
frontier. The 4,096 cap is nominal: truncation retains every zero-cost state
(any number, `zero_cost_excess` reports how far the zero-cost lane exceeds the
cap) plus positive-cost states to the cap, evicting nothing from the zero-cost
lane, and `truncated` discloses that the positive-cost frontier was cut. A
later exact board can re-seed from the unknown lane, which is the intentional
rejoin mechanism.

See [`alignment loop`](../../src/openers/recognition/align.rs).

A missing observation does not increment unknown cost. It retains the state and
may advance one model `Lock` for zero cost before epsilon closure. It is missing
evidence, not proof of a player or catalog deviation.

See [`consume_missing_observation`](../../src/openers/recognition/align/frontier.rs).

### Current Edit Costs

| `EditCosts` field | Default | Charged for |
| --- | ---: | --- |
| `synchronous` | 0 | Identical observed and expected keys |
| `substitute_letter` | 3 | Each differing pair of known letters on shared occupancy |
| `cell_mismatch` | 1 | Each cell occupied on only one side |
| `observed_only` | 5 | Observed lock without consumed model lock |
| `model_only` | 4 | Consumed model lock without observation |
| `identity_opaque_entry` | 2 | Entry to opaque state, and a consumed bridge transition |
| `unknown_per_lock` | 6 | Each present observation in the unknown lane |

These values are live defaults in
[`cost.rs`](../../src/openers/recognition/cost.rs). The cell/letter
comparison is defined by [`edit_cost`](../../src/openers/recognition/cost.rs).

The score is an edit score, never a probability or confidence. Do not turn a
low score, a high margin, an untruncated run, or an identity-preserving bridge
into a probability claim.

### Attribution Rules

The frontier records cost, edit operations, and opaque steps. A hypothesis that
has traversed an opaque step is emitted with `identityFrozen: true`; hypothesis
ordering then uses the unresolved-identity name for deterministic ordering.

See [`Frontier` entry rules](../../src/openers/recognition/align/frontier.rs)
and [`identity ordering`](../../src/openers/recognition/align.rs).

For one record, equal-cost alternatives retain fewer mismatches first, then the
lowest state id. Across records, results sort by total cost then record id. Route
recognition does not preserve an arbitrary confirmation primary.

See [`final evidence selection`](../../src/openers/recognition/align/evidence.rs).

## Output And Interpretation

`RoundRecognition` is attached to `OpenerRoundAnalysis` and serialized in
camelCase. It includes:

| Field | Interpretation |
| --- | --- |
| `retrievalBounded` | Always true for this production recognizer |
| `shortlistSize` | Number of selected identities; complete and uncapped for a confirmed survivor set, otherwise capped at 24 for weak assessment evidence |
| `truncated` | Shortlist, seed/reseed, or active-frontier truncation occurred |
| `shortlistCompileSkipped` | Shortlisted records excluded by compile failure/budget |
| `perLock` | Cost, unknown baseline, active states, exact hits, opaque entries per observed lock |
| `hypotheses` | Ranked record identities with total cost, margin, edit ops, and identity freeze state |
| `unknownCost` | Final all-unknown lane cost |
| `editCosts` | The cost schedule actually used |

See [`RoundRecognition`](../../src/openers/recognition/round.rs) and
its construction in [`recognize_round`](../../src/openers/recognition/round.rs).

`recognition: null` is a valid output. In particular, the empty-round WASM
contract serializes empty assessments/policies and null catalogued-board match,
report projection, guide, and recognition. The WASM test also pins the output's
camelCase evidence fields.

See [`WASM output contract test`](../../src/wasm.rs).

For presentation, a singleton catalogued-board match is the only identity
projection. Route hypotheses are bounded evidence, not an instruction to choose a
primary from a tie or claim source intent.

## Evidence, Census, And Hard Gates

### Census G1

The full-catalog census is test-only. It records compile budgets, state and
transition totals, legality outcomes, graph size/timing, collision buckets,
epsilon closure, and active-product-state maxima. The writer emits
`census.json` and `summary.md`.

See [`CompileCensus`](../../src/openers/recognition/census.rs) and
[`census writer`](../../src/openers/recognition/evidence.rs).

G1 fails when any of these conditions is true:

1. The epsilon graph is cyclic.
2. No lettered catalog edge compiled.
3. Direct-impossible edges exceed 2% of lettered edges.
4. Any budget-exceeded edge is not an accepted exclusion.
5. Exact-key bucket mean is non-finite.
6. Resident graph bytes were not recorded.

See [`g1_failures`](../../src/openers/recognition/census.rs).

The G1 populations are the pre-exposure census classes. Clauses 2 and 3 use
`lettered_edges`, which counts only non-bridge-exposed edges; clause 4 uses
`budget_exceeded_edges`, likewise bridge-exposed budget edges excluded; and
direct-impossible edges are the non-bridge-exposed class. Bridge-exposed
impossible, budget, and frame-inconsistent edges are recorded in separate
counters and do not enter the lettered/direct-impossible/budget populations
that G1 gates. See
[`direct_impossible` and `lettered_edge`](../../src/openers/recognition/census.rs).

Recorded bridge-exposed receipt: 14 impossible / 0 budget-exceeded / 0
frame-inconsistent edges, from the locally recorded census at
`evidence-out/opener-recognition-census/summary.md`.

The only accepted budget exclusions are `sasasa123-634:4` and
`sasasa123-933:2`. The source compares record id and node id when filtering
budget failures. It retains and reports those exclusions; it does not convert
them into successful compilation or justify additional exceptions.

See [`accepted exclusions`](../../src/openers/recognition/census.rs) and
[`budget summary`](../../src/openers/recognition/census.rs).

Do not weaken the authored G1 gates to accommodate a catalog, bridge, or
performance regression. Improve the catalog/frame semantics or make an explicit
reviewed budget decision instead.

### Behavioral Battery

The test-only behavioral battery writes `battery.json` and `summary.md`. Its
hard outcomes require legal zero-cost walks, substitution retention, missing
retention, nonzero hybrid stitching, zero-cost transpositions, unknown-winning
negatives, nonzero rejoin, and no zero-cost eviction on legal walks.

See [`HardOutcomes`](../../src/openers/recognition/battery/report.rs)
and [`battery report`](../../src/openers/recognition/battery/report.rs).

The report also exposes all tuning observables: active-product maximum, seeds,
truncations, zero-cost evictions/excess, per-lock evidence, skipped records,
synthesis limits, and slow alignments. Use those facts instead of inferring
quality from a single top hypothesis.

### Collision Exporter Boundary

Collision evidence is compiled only under `#[cfg(test)]`; the recognition module
does not expose it in normal Rust or WASM builds. The exporter reads an explicit
request, asset, and output path from `OPENER_COLLISION_REQUEST`,
`OPENER_COLLISION_ASSET`, and `OPENER_COLLISION_OUT`, compiles using the live
`CompileBudget`, and writes an evidence JSON document.

See [`test-only module boundary`](../../src/openers/recognition.rs) and
[`collision exporter`](../../src/openers/recognition/collision_evidence/export.rs).

This exporter is for offline collision review. It is not a public attribution
API, a WASM feature, or an instruction to disclose individual model-attribution
results in user-facing material.

## Tuning Knobs And Required Measurements

| Change surface | Changes | Does not change | Measure before accepting |
| --- | --- | --- | --- |
| Catalog records, frames, placements | Catalog coverage and legal graph range | The G1 standard | Full census: G1, direct-impossible rate, frame inconsistencies, bridges, graph bytes/time |
| Phase match limit, grey threshold | Assessment match depth and grey progress | Exact edit semantics | Candidate composition, assessment stability |
| Route candidate limits | How many identities can enter route recognition | Confirmation semantics and exact edit semantics | Candidate composition and `truncated` disclosures |
| Catalogued tree boards and ordinal | Which records may enter or leave the confirmation survivor set | Route evidence, compiler legality, and G1 | Tie/null/rejoin/post-PC confirmation fixtures |
| `relaxDepth` | Attack-gap policy for per-lock index thresholds | Confirmation, compiler legality, G1 | Policy booleans and existing attack expectations |
| Compile budgets | Which shortlisted records/paths can compile and graph size | Catalog identity or score meaning | G1 plus per-edge states, DFS histogram, skipped records, compile time, resident bytes |
| Seed and active-product budgets | Alignment exploration capacity and truncation risk | Seed ties are still retained | Seeds, reseed truncations, active-state maximum, zero-cost evictions/excess, rejoin/recall battery gates |
| Edit costs | Which legal attribution wins and edit explanation | Probability calibration | Full behavioral battery, cost-specific retention, margins, negative unknown wins |
| UI treatment of `truncated`, `identityFrozen`, margins | How uncertainty/evidence is displayed | Search, compile, or alignment result | Payload snapshots and browser/replay presentation checks |

There is no standalone confidence threshold. The output has evidence flags and
bounded-search disclosures, not confidence scores.

For range changes, begin with a census and frame diagnostics. For attribution
changes, begin with the behavioral battery and examine whether the result became
truncated or identity-frozen. For report/UI-only changes, retain payload and
replay coverage, then inspect the affected presentation surface.

## Historical Results, Not Defaults

<details>
<summary>Path A-F receipts</summary>

Path A's original census failed the direct-impossible gate; Path B stopped at
its source-backed coverage gate. Neither is a current runtime dependency.

Path C introduced declared-frame/physical-shadow compilation and passed the 2%
clause at 1.9956%, establishing the current compile limits and separately
accepted exclusions. Path D added exact alignment, raw edit scores, rejoin,
opaque evidence, and the battery. Path E productized route recognition; Path F
added bridge exposure and local WASM cutover. These are historical receipts, not
defaults or probability calibration.

</details>

## Current Receipt

The confirmation follow-up is complete locally: Fusion
`939e76a8d76545d28b0e08f642f05f071188f181`
(`feat(openers): confirm catalogued boards by lock ordinal`) passed review gate
`.omo/review-gate/approved-26cf8a83bcd32792.json` for reviewed diff SHA-256
`26cf8a83bcd327925aa63fbd63b124d410b694c2b1c456f4405c486bff4829f1`.
Gates: catalogued-match 10, phase 7, non-ignored opener tests 106 passed / 5
ignored, installed-catalog integration 1, unchanged full census PASS, behavioral
battery PASS, and wasm32 PASS at the known 15-warning baseline. Strict Clippy has
only unrelated pre-existing backlog. Mosaic is
`fa5274edc1a1beb2bc993c5b7d9af3afaa6b0ea4`; the byte-stable two-build WASM
SHA-256 is `3787ed45e99fd1fe067e268adc8d446eba2cbb6788d1b9afe213a82b52807cf5`.
No push, asset ingest, fold, live asset change, or protected-file overwrite is
implied by this receipt.

See [root `STATE.md`](../../../STATE.md).

## Verification Commands

Run these from `fusion-engine/` when source or catalog behavior changes:

```bash
source "$HOME/.cargo/env" && CLOUD_EXEC_SKIP=1 cargo metadata --no-deps --format-version 1
source "$HOME/.cargo/env" && CLOUD_EXEC_SKIP=1 cargo test --lib
source "$HOME/.cargo/env" && CLOUD_EXEC_SKIP=1 cargo clippy -- -D warnings
```

`cargo test --lib` runs the ordinary unit tests (including the mini-catalog
battery) but skips the two `#[ignore]` gates by default: the full-catalog
census and the full behavioral battery. Run them explicitly when tuning
changes compilation, alignment, or cost behavior. The census needs no output
env var; the battery writes its report only to the directory named by
`OPENER_BATTERY_OUT` and fails without it. To avoid polluting the workspace,
point the cargo target directory at a scratch directory while they run.

```bash
source "$HOME/.cargo/env" && CLOUD_EXEC_SKIP=1 \
  CARGO_TARGET_DIR=/tmp/fusion-census-target \
  OPENER_CENSUS_OUT=evidence-out/opener-recognition-census \
  cargo test --release --lib openers::recognition::tests::full_catalog_census -- \
    --ignored --exact --test-threads=1 --nocapture
source "$HOME/.cargo/env" && CLOUD_EXEC_SKIP=1 \
  CARGO_TARGET_DIR=/tmp/fusion-battery-target \
  OPENER_BATTERY_OUT=evidence-out/opener-recognition-battery \
  cargo test --release --lib \
    openers::recognition::battery::tests::behavioral_battery -- \
    --ignored --exact --test-threads=1 --nocapture
```

Both tests read the frozen catalog that hydration installs at
`fixtures/openers/catalog-full.json` (catalog SHA-256
`5abc5168ee0f7d54d4ced9378aa244abfa7c1cda82d4ede08fa9fcff9d7d8fe0`, the
current public opener release). Run `sh scripts/hydrate-opener-tests.sh` first;
no sibling application checkout is needed. The census gate additionally joins
the retained path-A report at
`evidence-out/opener-recognition-census/census.json`, which is not published
with the source, and fails loudly when it is missing; the battery gate needs no
retained report. Omitting `OPENER_CENSUS_OUT` still prints the census G1
verdict, but the command above sets it so `census.json` and `summary.md` are
refreshed. The battery hard gates are asserted in-process, so a passing exit
code is the gate result. The default `cargo test --lib` suite needs neither
retained report: census compilation itself never reads one.

`cargo clippy -- -D warnings` is the lint gate for this tree. When it reports a
lint inside `src/movegen.rs` or the allowlisted report schema file in
`src/versus/report.rs`, treat it as unrelated pre-existing backlog rather than
evidence about recognizer changes.

For an intentional WASM-boundary change, also run:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
wasm-pack build --target web --release --out-name fusion_wasm -- --no-default-features --features wasm
```

For recognizer tuning, inspect the generated census and battery outputs, and
exercise `install_opener_runtime` followed by `analyze_opener_round` through the
same WASM payload boundary used by the client. A successful compile alone is
not recognition evidence.
