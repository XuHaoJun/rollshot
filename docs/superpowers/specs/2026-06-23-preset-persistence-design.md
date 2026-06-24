# Preset Persistence Design (Sub-project 5)

**Parent:** `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md`,
§12 Delivery Decomposition, item 5 ("Preset persistence").

**Predecessor:** Sub-project 4 (Bounded Agent Core),
`docs/superpowers/handoffs/2026-06-23-bounded-agent-core.md`.

**Crate:** new `rollshot-preset`.

## 1. Summary

Sub-project 5 gives the Smart Redaction Workbench a durable home for the
artifacts a user keeps: **presets** and the **immutable automation revisions**
behind them. It persists each preset's metadata, an append-only set of
immutable `AutomationRevision` records (each wrapping a validated automation
artifact), and the user's **active revision selection**. Loading a revision
revalidates it against the installed automation schemas before it can be used.

Storage is file-based JSON under a per-platform config directory, written
atomically. The crate is framework-neutral and headless: it owns no UI,
windowing, capture, provider, or agent code.

## 2. Scope

### 2.1 In scope

- A `Preset` record: id, name, original intent, active revision selection,
  timestamps.
- An immutable `AutomationRevision` record wrapping
  `rollshot_automation::ValidatedAutomation` plus provenance and lineage
  (`parent_id`).
- Active-revision selection with integrity enforcement.
- Revalidation of a loaded revision via
  `rollshot_automation::ensure_compatible` before use.
- Atomic, crash-safe file writes and a concurrency story for multi-process
  access (daemon + app).
- A typed error model.

### 2.2 Explicitly out of scope (decision)

Parent §6 lists `AgentSession`, `AgentRun`, `AutomationRun`, `EditProposal`,
and `ReviewDecision`. The parent §12 SP5 bullet also names "privacy-safe
session/run persistence." **This sub-project deliberately defers all
session/run persistence to Sub-project 6 (Preset Workbench).**

Rationale:

- A preset and its accepted revisions are the durable, reusable product;
  an agent session is transient working state whose only consumer is
  **session resume**, and the only thing that exercises resume is the
  Workbench UI, which does not exist yet. Building persist → privacy-DTO →
  revalidate-on-load → resume before its consumer is speculative work.
- Sessions are *state*, not *config*; they do not belong next to presets, and
  giving them a separate `state_dir` tree is better designed once the UI
  defines what resume restores.
- Deferring keeps SP5 dependency-light: it depends only on
  `rollshot-automation` and never touches `rollshot-agent`, preserving that
  crate's stated in-memory-only posture.
- Persisting less sensitive conversational text by default is the
  privacy-safer choice (parent §9.5).

Also out of scope: `EditProposal` / `ReviewDecision` / image-`AutomationRun`
persistence (produced and reviewed in SP6/SP7); **migration** of automation
artifacts across schema versions (SP5 *detects* incompatibility, it does not
migrate); UI; at-rest encryption.

The on-disk format reserves a `state_dir/rollshot/sessions/` location and a
`provenance.source_run_ref` field as forward hooks so SP6 can add session
linkage without a schema migration.

## 3. Relationship to existing code

SP5 reuses two facts established by SP4 / SP3 rather than reinventing them:

- **`rollshot_automation::ValidatedAutomation`** is already fully
  `serde`-serializable and already contains exactly what must be persisted:
  canonical `source`, `workflow_ir` (carrying `capability_manifest` and
  `static_cost`), all four schema versions
  (`language_schema_version`, `ir_schema_version`, `capability_api_version`,
  `output_schema_version`), `validation_limits`, and `validation_summary`.
  It is the persisted automation artifact verbatim.
- **`rollshot_automation::ensure_compatible(&ValidatedAutomation)`** already
  implements the "validated artifacts are revalidated before execution"
  invariant: it checks the four installed schema versions and then rebuilds
  the artifact from its canonical source and asserts structural equality,
  returning `CompatibilityError` on any mismatch. SP5's load path calls it.

## 4. Crate boundary and layout

New crate `rollshot-preset`:

- `unsafe_code = "forbid"`.
- No UI, windowing, capture, provider, or agent dependency.
- Dependency direction: `rollshot-preset → rollshot-automation` only (one-way;
  `rollshot-automation` does not depend on `rollshot-preset`, so no cycle).
- The store accepts an **injected `root: PathBuf`**. The crate never resolves a
  real home/config path and never reads environment variables, so unit tests
  run entirely against a temporary directory.

Modules:

- `domain.rs` — `Preset`, `AutomationRevision`, `RevisionProvenance`,
  `PresetId`, `RevisionId`, `PresetSummary`, `RevisionSummary`.
- `store.rs` — `PresetStore` and all filesystem operations.
- `io.rs` — atomic write helper and advisory-lock helper.
- `error.rs` — `StoreError`, `EntityKind`.

Directory resolution lives at the **product edge** (`rollshot-app`), which owns
the `etcetera` dependency, resolves the config/state roots, and constructs the
`PresetStore`. See §6.

## 5. Data model

IDs and timestamps are **caller-supplied**, keeping the crate pure and
deterministic under test (mirroring `rollshot-agent`'s caller-supplied
`SessionId::new`). The product layer generates opaque UUID strings and RFC 3339
timestamps; the crate treats them as opaque values.

```text
PresetId(String)        // opaque UUID
RevisionId(String)      // opaque UUID

Preset
  store_schema_version: u16
  id: PresetId
  name: String
  original_intent: String
  active_revision_id: Option<RevisionId>   // None until first accepted revision
  created_at: String                       // RFC 3339
  updated_at: String

AutomationRevision                         // immutable / write-once
  store_schema_version: u16
  id: RevisionId
  preset_id: PresetId
  parent_id: Option<RevisionId>            // lineage for a future version canvas
  created_at: String
  provenance: RevisionProvenance
  artifact: ValidatedAutomation            // from rollshot-automation, verbatim

RevisionProvenance
  origin: RevisionOrigin                   // AgentRun | Import | Manual
  note: Option<String>
  source_run_ref: Option<String>           // reserved opaque hook for SP6 linkage
```

- `AutomationRevision` is immutable: there is no API to mutate or overwrite an
  existing revision. Every accepted agent modification is a new revision.
- `active_revision_id` is `Option` because a freshly created preset has no
  accepted revision yet (parent §6.2: "only user acceptance makes a draft
  revision active").
- `parent_id` preserves branch lineage. The first-release model is linear; no
  merge semantics.
- `store_schema_version` versions the SP5 *file envelope*, independently of the
  automation schema versions embedded in the artifact. It lets the on-disk
  format evolve on its own track.

## 6. On-disk layout and directory resolution

```text
<root>/presets/<preset-id>/
  preset.json                 # the Preset record
  revisions/<rev-id>.json     # one immutable AutomationRevision per file
```

`<root>` is injected. The product edge resolves it through the **single shared
resolver** `rollshot_app::daemon::config::rollshot_config_dir()`, so presets
nest under the *same* root as the daemon's `config.toml` and `daemon.lock` —
one rollshot config root per machine. That resolver is upgraded from
`dirs::config_dir()` to the **etcetera** XDG base strategy (rollshot is not yet
publicly released, so no path migration is needed), matching the reference
`opencode` layout:

- Linux / macOS: `~/.config/rollshot` (honors `$XDG_CONFIG_HOME`); presets at
  `~/.config/rollshot/presets/`.
- Windows: native `%APPDATA%\rollshot` (etcetera's `choose_base_strategy` uses
  the Windows strategy there; rollshot does not target Windows today, so this
  divergence is acceptable).

The preset root is `rollshot_config_dir()?.join("presets")`. The reserved
future session location is `state_dir()/rollshot/sessions/` (Linux/macOS
`~/.local/state/rollshot/sessions`), kept separate from the config tree so
transient state never mixes with durable config. SP5 does not create or use it.

Presets are stored as **config** rather than **data** because a preset is a
JavaScript automation the user authors and reuses — configuration, not
app-generated data.

## 7. Store API

```rust
PresetStore::open(root: PathBuf) -> Self            // lazy; performs no scan

// Preset lifecycle
fn create_preset(&self, id, name, original_intent, now) -> Result<Preset>
fn list_presets(&self) -> Result<Vec<PresetSummary>>
fn load_preset(&self, id: &PresetId) -> Result<Preset>      // metadata only
fn rename_preset(&self, id: &PresetId, new_name, now) -> Result<()>
fn delete_preset(&self, id: &PresetId) -> Result<()>

// Revisions (append-only, immutable)
fn add_revision(&self, preset_id, id, parent_id, artifact, provenance, now)
    -> Result<AutomationRevision>                   // does NOT auto-activate
fn list_revisions(&self, preset_id: &PresetId) -> Result<Vec<RevisionSummary>>
fn load_revision(&self, preset_id, rev_id) -> Result<AutomationRevision>
fn load_active_revision(&self, preset_id) -> Result<AutomationRevision>

// Active selection
fn set_active_revision(&self, preset_id, rev_id) -> Result<()>  // integrity-checked
```

`add_revision` and `set_active_revision` are intentionally **separate**.
Accepting a draft in the product is "`add_revision` then `set_active_revision`";
this is what enforces "only user acceptance makes a draft revision active."
`load_revision` and `load_active_revision` run the §8 revalidation before
returning.

`PresetSummary` / `RevisionSummary` are lightweight projections (ids, name,
timestamps, active flag) for listing without deserializing every artifact.

## 8. Revalidate-on-load invariant

`load_revision` and `load_active_revision`:

1. Read and deserialize the revision JSON, including its embedded
   `ValidatedAutomation`.
2. Call `rollshot_automation::ensure_compatible(&artifact)`, which
   - checks the four installed schema versions against the artifact's, and
   - rebuilds the artifact from its canonical `source` and asserts structural
     equality.
3. On any failure, return `StoreError::Incompatible(CompatibilityError)`.

This yields, for free from the automation crate: tamper detection (a persisted
artifact whose stored fields no longer match a fresh validation of its source),
and stale-schema detection (a revision authored under an older schema than the
installed one). SP5 surfaces these as typed errors for the product to present;
it does not migrate artifacts.

## 9. Atomic writes, crash-safety, concurrency

- **Atomic write:** serialize to `<file>.tmp` in the same directory, `fsync`
  the temp file, `rename` it over the destination, then best-effort `fsync` the
  directory. A reader sees either the previous file or the new one, never a
  partial write. Stray `.tmp` files are ignored when reading and listing.
- **Accept-draft ordering:** write the revision file (fsynced) **before**
  updating `preset.json`'s `active_revision_id`. A crash in between leaves a
  harmless orphan revision file plus the previous valid active pointer.
- **Concurrency:** immutable revision files have unique ids and are therefore
  conflict-free across writers. Only `preset.json` mutations (`active`,
  `name`) need serialization; they take a per-preset-directory **advisory file
  lock** (the same approach as `opencode`'s `Flock`, and the existing daemon
  `InstanceGuard`). This uses `fs4`, already a workspace dependency — no new
  crate.

## 10. Error model

```rust
enum StoreError {
  Io(std::io::Error),
  Serialize(serde_json::Error),
  NotFound { kind: EntityKind, id: String },
  Incompatible(rollshot_automation::CompatibilityError),
  Integrity(String),       // active_revision_id missing or belongs to another preset
  RevisionExists(String),  // attempt to overwrite an immutable revision id
  Corrupt { path: PathBuf, detail: String },
}

enum EntityKind { Preset, Revision }
```

`set_active_revision` returns `Integrity` if the target revision does not exist
or belongs to another preset. `add_revision` returns `RevisionExists` if the id
is already present (immutability guard). Malformed JSON or a violated on-disk
invariant returns `Corrupt`.

## 11. Privacy and retention

- SP5 persists exactly the parent §9.5 "persist by default" set that falls in
  scope: preset metadata, accepted automation source and Workflow IR (inside
  the artifact), and revision metadata. It persists none of the §9.5
  "do not persist" set (screenshots, raw OCR, raw tool results, provider
  bodies, sensitive attachments) — those never enter this crate.
- `Preset.original_intent` is user-authored natural-language text persisted per
  parent §6.1. It can contain whatever the user typed; it is stored in plain
  JSON. At-rest encryption is out of scope for SP5.
- The crate emits no diagnostics containing automation source, intent text, or
  artifact contents. Any `tracing` it adds uses stable `rollshot::*` targets
  with privacy-safe structured fields only (counts, ids as opaque strings,
  durations, error kinds) — consistent with parent §9.6.

## 12. Failure semantics

- **Not found:** missing preset or revision → `NotFound`.
- **Incompatible artifact:** schema mismatch or artifact/source mismatch on
  load → `Incompatible`; the product offers re-creation, not silent use.
- **Integrity violation:** dangling or foreign `active_revision_id` →
  `Integrity`; the active pointer is never silently repaired.
- **Corruption:** unreadable/invalid JSON → `Corrupt` with the offending path.
- **I/O / serialization:** surfaced as `Io` / `Serialize`.

No failure mode silently drops or rewrites an immutable revision.

## 13. Verification

The crate is pure and uses TDD; all tests inject a temporary-directory root and
touch no real home or environment.

- Round-trip: `create_preset` → `add_revision` → `set_active_revision` →
  reload; `load_active_revision` passes `ensure_compatible`.
- Immutability: re-adding an existing revision id → `RevisionExists`; revision
  files are never rewritten.
- Active lifecycle: a new preset has `active_revision_id == None`; activation
  sets it; `set_active_revision` to a missing or foreign revision → `Integrity`.
- Revalidate-on-load: tampering a persisted artifact's stored fields so a
  rebuild no longer matches → `Incompatible`; a simulated older schema version
  → `Incompatible`.
- Atomic write: a leftover `.tmp` file is ignored by read and list; no partial
  document is ever returned.
- Listing: `list_presets` / `list_revisions` return summaries without failing
  on a half-written `.tmp`.

Standard gates: `rtk cargo test -p rollshot-preset`, `rtk cargo fmt --check`,
`rtk cargo clippy -p rollshot-preset --all-targets -- -D warnings`, and a
workspace test/clippy pass.

## 14. Out of scope and SP6 seams

- Session/run persistence — deferred to SP6; reserved at
  `state_dir()/rollshot/sessions/`.
- `EditProposal` / `ReviewDecision` / image-`AutomationRun` persistence — SP6/SP7.
- Automation-artifact migration across schema versions — SP5 detects
  incompatibility only.
- UI and at-rest encryption.
- `provenance.source_run_ref` is the reserved no-migration hook for future
  session linkage.

## 15. Success criteria

SP5 is complete when, against an injected store root, the product can:

1. Create a preset and list it.
2. Add an immutable automation revision from a `ValidatedAutomation` without it
   becoming active.
3. Activate a revision, with selection integrity enforced.
4. Reload a preset's active revision across process restarts, with the artifact
   revalidated via `ensure_compatible` before use.
5. Receive typed errors for not-found, incompatible, integrity, and corruption
   cases, with no silent repair or loss of an immutable revision.
6. Survive a crash mid-write without ever reading a partial or
   internally-inconsistent record.
