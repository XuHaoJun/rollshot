# JS Frontend / Parser Spike - Findings

## Status

- Lifecycle: active
- Decision owner: Task 6 (joint MSRV resolution)
- Started: 2026-06-20
- Last updated: 2026-06-20

## Decision

Which Rust parser backs the restricted-subset validator + Workflow IR normalizer,
and what MSRV does it impose? Final pick deferred to Task 6.

## Environment

- Platform: Linux 6.8.0 x86_64
- Rust: stable 1.89.0 (workspace floor), 1.88.0 also available
- Spike crate: spikes/js-frontend/ with empty [workspace] table (isolated)
- Pinned versions: oxc 0.137.0, swc_ecma_parser 41.1.1, tree-sitter 0.26.9,
  boa_parser/boa_ast/boa_interner 0.21.1
- Crates.io maintenance data fetched: 2026-06-20

## Risk Results

| Risk | Gate | Evidence | Result | Notes |
|------|------|----------|--------|-------|
| Parse valid fixture without error | hard | automated | PASS all 4 | valid_detector.js accepted |
| Reject all 8 §5.2 constructs with span | hard | automated | PASS all 4 | 9/9 correct each |
| IR extraction feasibility | soft | automated | PASS all 4 | [rollshot.ocr, .filter, .map] |
| Build on workspace floor 1.89 | soft | compile | PASS treesitter/boa/swc; FAIL oxc | oxc 0.137.0 requires Rust 1.94.0 |
| License compatible | soft | review | PASS all 4 | oxc=MIT, swc=Apache-2.0, ts=MIT, boa=MIT |
| Maintenance active | soft | crates.io | PASS all 4 | recent releases |
| macOS C-build parity | conditional | UNTESTED | — | only if tree-sitter is finalist |

## Observations

### Step 1: Scaffold

Pinned versions at execution time (2026-06-20):

| Candidate | Pinned version | crates.io max_stable |
|-----------|---------------|---------------------|
| oxc | 0.137.0 | 0.137.0 |
| swc | swc_ecma_parser 41.1.1 / swc_common 23.0.2 / swc_ecma_ast 25.0.0 | matching |
| tree-sitter | 0.26.9 / tree-sitter-javascript 0.25.0 | matching |
| boa | boa_parser 0.21.1 / boa_ast 0.21.1 / boa_interner 0.21.1 | matching |

SWC version-matching: swc crates do NOT declare rust_version. The set
swc_ecma_parser 41.1.1 + swc_common 23.0.2 + swc_ecma_ast 25.0.0
resolved and built without issues.

### Step 2: Fixtures

Created fixtures/valid_detector.js and 8 reject fixtures: var, while,
dynamic access rollshot[k](), Reflect.ownKeys, recursion (fn self-call),
class declaration, escaping closure (sink.push inside arrow), generator function*.

### Step 3-5: Parse + Span Quality + Walker + IR Extraction

All four candidates: accepted valid_detector.js, rejected all 8 fixtures
(9/9 correct). IR sequence [rollshot.ocr, .filter, .map] extracted by all.

Span quality:
- oxc: Byte-exact spans (start..end bytes) with line+col. Best ergonomics.
  Arena allocator required but not a burden for validation.
- swc: Global SourceMap BytePos accumulates across all registered files.
  Raw byte values are large (220..235 for file byte 23..38). Requires
  SourceMap lookup for user-facing positions. Added ergonomic cost.
- tree-sitter: Line+column (1-indexed) via start_position/end_position.
  CST cursor uses node.kind() string matching rather than typed enum arms.
  dynamic_member_access reported twice for one fixture (traversal artifact, harmless).
- boa: Expression spans accurate where Spanned is implemented. VarDeclaration
  and WhileLoop lack Spanned in boa_ast 0.21 (spans report 0:0 for those
  constructs). Identifier names require Interner lookup. More involved API.

### Step 6: MSRV / License / Maintenance / Binary Cost

MSRV probed by cargo +1.89.0 build --features <candidate>:

| Candidate | Declared rust_version | Builds on 1.89? | Min Rust imposed |
|-----------|----------------------|-----------------|-----------------|
| oxc 0.137.0 | 1.94.0 | NO (resolver error on 1.89) | 1.94.0 |
| swc 41.1.1 | not declared | YES | ~1.86 empirical |
| tree-sitter 0.26.9 | 1.77 | YES | 1.77 |
| boa 0.21.1 | 1.88.0 | YES (1.89 >= 1.88) | 1.88.0 |

Note: pure-Rust and low-MSRV are orthogonal. tree-sitter (C-backed) has
the lowest MSRV (1.77). Latest oxc (pure Rust) has the highest (1.94).

License: oxc=MIT, swc=Apache-2.0, tree-sitter=MIT+C grammar, boa=MIT/Unlicense.
All permissive and compatible with rollshot.

Maintenance activity (crates.io, 2026-06-20):
- oxc_parser: updated 2026-06-18, 1,127,270 recent downloads
- swc_ecma_parser: updated 2026-06-18, 4,761,498 recent downloads
- tree-sitter: updated 2026-05-19, 9,099,231 recent downloads
- boa_parser: updated 2026-03-29, 1,059,561 recent downloads

Binary size + dep tree (release build):
- oxc 0.137.0: 1.5 MiB, ~130 cargo tree lines
- swc: 2.8 MiB, ~208 cargo tree lines
- tree-sitter 0.26.9: 1.0 MiB, ~30 cargo tree lines
- boa 0.21.1: 3.6 MiB, ~126 cargo tree lines

### Step 7: macOS C-Build

tree-sitter IS a finalist. Controller must run macOS CI (Spikes macos-14)
to confirm --features treesitter builds. Pure-Rust candidates need no macOS check.

## Final Recommendation

### Decision Matrix

| Dimension | OXC | SWC | tree-sitter | Boa |
|-----------|-----|-----|-------------|-----|
| §5.2 coverage (9/9) | YES | YES | YES | YES |
| Span quality | byte-exact | poor (global accum.) | line:col CST | partial (stmt=0:0) |
| IR feasibility | YES | YES | YES | YES |
| MSRV imposed | 1.94.0 | ~1.86 | 1.77 | 1.88.0 |
| Builds on 1.89 | NO | YES | YES | YES |
| License | MIT | Apache-2.0 | MIT+C | MIT/Unlicense |
| Maintenance | High | High | High | Moderate |
| Binary size | 1.5 MiB | 2.8 MiB | 1.0 MiB | 3.6 MiB |
| Dep count | ~130 | ~208 | ~30 | ~126 |
| Traversal ergonomics | Excellent (typed AST) | Good | Moderate (CST strings) | Moderate (interner) |
| Pure Rust | YES | YES | NO (C grammar) | YES |
| macOS parity | low risk | low risk | NEEDS CI gate | low risk |

### Shortlist (ranked)

1. OXC -- best span ergonomics (byte-exact), typed arena AST, fastest-growing
   ecosystem. Cost: raises workspace floor from 1.89 to 1.94 (primary risk
   for Task 6 to resolve).

2. tree-sitter -- lowest MSRV (1.77), smallest footprint (~30 deps, 1.0 MiB),
   accurate line:col spans. Cost: C build dependency; macOS parity needs CI
   gate; CST traversal more verbose.

3. SWC -- 9/9 constructs, builds on 1.89, Apache-2.0. Span ergonomics require
   SourceMap overhead for user-facing positions. Largest dep tree.

4. Boa -- 9/9 constructs, builds on 1.89 (MSRV 1.88). Span coverage incomplete
   for statement constructs. No runtime synergy (Task 2 chose rquickjs).
   Highest binary cost.

### MSRV Evidence for Task 6

- Floor can reach 1.94: OXC is the clear pick.
- Floor must stay <= 1.89: shortlist is tree-sitter (after macOS CI gate) and SWC.
- Boa at 1.88 viable but has span gaps and lower ergonomics.

Do NOT presume any specific MSRV target -- that is Task 6's call.
Per-candidate MSRV recorded above as evidence.

### Rejected Alternatives

- boa_engine (full runtime): boa_parser/boa_ast sufficient for parse-only use.
- Downgrading oxc: not attempted per brief (record MSRV as data, move on).

### Fallback Triggers

- Floor <= 1.89 AND Apache-2.0 unacceptable: tree-sitter (after macOS CI).
- tree-sitter macOS C-build fails: SWC.
- All pure-Rust <= 1.89 options ruled out: revisit oxc with floor bump.

### Controller Action Required

tree-sitter IS a finalist. Controller must run macOS CI Step 7 to confirm
spikes/js-frontend builds with --features treesitter on macos-14.
