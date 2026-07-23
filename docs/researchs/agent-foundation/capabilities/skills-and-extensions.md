# Skills and extensions comparison

Status: Reviewed

Research date: 2026-07-22 (Asia/Taipei)
**Umbrella revision:** 1

Compared revisions:

- Rollshot: `18d6395d2397f29c5c5339390676fb13f64b7235`
- Pi: `dd6bea41efa8caa7a10fe5a6401676dc5699f83f`
- oh-my-pi (OMP): `7b141199d524b859c357fc89654f10b62b9f3df1`
  (`v17.0.7`)
- Codex: `4a443994bd12f49f2f08b21a2f224d9d42b9e734`
- Claude Code: `2ca5ddabfed5f220812ea11f029eda03b21bc4c1`

Evidence mode: static inspection of the pinned local source trees, their tests,
and the Reviewed Round 1 profiles. Reference checkouts have no code-review graph
coverage, so bounded source inspection followed the required `0 nodes / 0 files`
graph checks. Claims marked **inference** were not runtime-tested. [E0, E1]

## 1. Problem and workload pressure

“Skill” is an overloaded word. It can mean a Markdown instruction, a package of
instructions and resources, an available model capability, a permission grant,
or executable host code. Those are different security and lifecycle objects.
Rollshot needs a vocabulary that does not let discovery or prompt injection
silently become execution authority.

This comparison tests the designs against the umbrella workloads rather than
against a generic coding-agent marketplace. [E2]

1. **Smart Redaction** is one bounded, cancelable review run. A skill could hold
   redaction policy, review instructions, and examples. Consent, run budget,
   image access, proposal validation, and apply/reject remain Rollshot-owned.
   Durable general-purpose skill state is unnecessary.
2. **Action Guide** is a durable Rollshot project/revision around independent,
   bounded proposal tasks. Skills could describe task-specific detection or
   editing workflows, but the guide, revision, stale-result checks, and semantic
   event privacy boundary remain product state.
3. **Deferred brag-document plus Hyperframes** needs project inspection,
   versioned inputs, skill-to-skill handoff, instructions/resources, optional
   scripts, and multi-stage artifacts. It creates the strongest case for opaque
   package/resource identity and executor/orchestrator sources, but does not by
   itself justify a runtime extension marketplace or a general worker platform.

The core question is therefore not “does the system have skills?” It is:

> What is discovered, who authored and currently vouches for it, what the model
> may request, what Rollshot has granted, and which executor may perform the
> resulting operation?

## 2. Terms and non-negotiable safety invariant

| Term | Meaning in this document |
| --- | --- |
| **Instruction package** | Declarative guidance, metadata, and addressable resources. It may mention scripts; it is not itself an executor. |
| **Resource** | Text or bytes addressed relative to a package or by an opaque provider handle. |
| **Skill catalog** | Bounded metadata describing currently available packages. Catalog presence is availability only. |
| **Explicit invocation** | A user or model names a skill/package directly. |
| **Implicit invocation** | The host selects a skill from user input or other context without an exact user-supplied name. |
| **Extension module** | Executable host code that may register tools, hooks, providers, UI, or lifecycle behavior. |
| **Source authority** | The provider and trust domain responsible for resolving a package/resource handle. |
| **Grant** | Product-owned authorization for a concrete action, target, scope, and lifetime. |
| **Execution authority** | The tool executor/environment that can actually touch files, network, processes, images, or product state. |

**Safety invariant:** skill text, metadata, resource contents, invocation, and
catalog membership never authorize tools, filesystem, network, subprocesses,
image access, document mutation, or product permissions. A package script can
run only through an ordinary Rollshot tool/executor after policy evaluation,
permission checks, budget accounting, cancellation wiring, and result
validation. “The skill said to run it” is a request, never a grant.

This is stricter than systems where skill frontmatter can influence command
allow rules. It is intentional: Rollshot owns user consent and product
permissions, and extensibility must preserve that ownership. [E2, E12]

## 3. Current Rollshot baseline

Rollshot has no skill/package catalog, skill provider, skill invocation record,
skill snapshot, extension registry, or plugin registry in the six inspected
`rollshot-agent` core files. The graph search also returned no matching entity.
This is an exact inspected-scope negative, not a repository-wide claim. [A:R0]

What exists is the safer substrate a future design should reuse:

- a typed, availability-aware tool registry;
- `AuthorizedModelInput`, keeping provider-bound input behind Rollshot checks;
- bounded run/session state, sixteen budget dimensions, cancellation, and
  explicit terminal states;
- a validate → dry-run → submit-for-review proposal path; and
- a product-owned `ImageDocument` operation boundary rather than arbitrary
  model mutation. [E3]

Consequently, adding a skill does not fill a missing authorization layer. It
adds a discovery/instruction/resource layer above authorization and execution.

## 4. Discovery, trust, metadata, catalog, and invocation

“Not found” and “unknown” cells below include an exact audit reference. They do
not generalize beyond that path/revision.

| System | Discovery and precedence | Trust boundary | Metadata/catalog disclosure | List/read/search | Explicit versus implicit invocation |
| --- | --- | --- | --- | --- | --- |
| **Pi** | Global `~/.pi/agent/skills`, compatibility `~/.agents/skills`, trusted project `.pi/skills`, project/ancestor `.agents/skills`, packages, settings, and CLI paths feed `loadSkills`; canonical realpaths deduplicate and first name wins. A directory containing `SKILL.md` is a skill root and stops recursive descent. [P1] | Project-local resources load only after project trust. Trust gates input loading; Pi explicitly does not sandbox extensions. [P2] | `disable-model-invocation` controls prompt visibility. A missing description rejects the skill; names above 64 characters and descriptions above 1,024 characters emit validation warnings but are still accepted. Metadata is rendered first with file paths, then the model is told to read the file. [P1, A:P3] | The prompt lists local entries and ordinary filesystem Read resolves content; the focused type audit found no source-authority/package/resource identity in this path. [P1, A:P1] | `/skill:name` explicitly rereads the current file and injects its body. Model invocation is implicit through metadata plus ordinary Read unless disabled. Explicit reread means invocation can observe post-catalog file changes. [P3] |
| **OMP** | A capability registry combines native, managed, Codex, Claude, Claude-plugin, OpenCode, GitHub, agents, OMP-plugin, and custom sources. Provider priority, include/ignore, enable/disable, realpath deduplication, and deterministic order select winners; managed entries are lowest priority. [O1] | Source level/provenance is retained, but the inspected `Skill`/protocol types do not bind an authority/package/resource tuple. `activeSkills` is a process-global selection snapshot, not an authorization grant. [O1, A:O1] | Frontmatter plus `_source` and level feed metadata into the prompt. Hidden/disable-model-invocation entries are excluded from prompt discovery but remain explicitly addressable. No aggregate metadata cap was found in the focused cap audit. [O1, A:O5] | Capability discovery/listing and `skill://` reading exist. A focused API-symbol audit found provider-labelled discovery comments/source fields and name-based `find`, but no skill search/query/catalog API or authority-routed search. `skill://` rejects absolute/lexical `..` and checks `path.resolve` containment. [O2, A:O4] | `/skill:` can appear at the beginning or within user input; explicit expansion rereads the file. Model invocation uses prompt metadata. Hidden skills remain explicit-only. [O1] |
| **Codex** | Host, executor, and orchestrator providers produce one catalog. Host discovery considers system/admin/plugin/repo/user roots with bounded traversal; executor sources are selected execution-environment roots; orchestrator entries are bounded MCP resources. [C1] | `SkillAuthority { kind, id }`, opaque package/resource IDs, and optional environment binding preserve provider authority. The provider contract explicitly forbids converting a resource into an ambient local path. [C1] | Enabled/prompt-visible catalog entries expose bounded name/description metadata. Budget is 2% of context, capped at 4,000 tokens; fallback is 8,000 characters. Main prompt content is capped at 8,000 bytes. [C2] | Provider API has `list/read/search`; all three inspected provider `search` implementations return empty results. Model tools expose only orchestrator `list/read`; there is no `search.rs`. Read recatalogs the authority/package before routing an opaque resource. [C1, A:C1] | Explicit mentions select and read main prompts. Separately, core code attributes an implicit invocation when a permitted skill's `SKILL.md` is read or a script below its `scripts/` directory is run. This detects model/tool behavior; it does not preselect content. The candidate selector remains shadow metrics. Host/executor metadata is prompt/world-state input, while model `list/read` says only orchestrator is supported. [C3] |
| **Claude Code** | Managed, user, project, added directories, legacy commands, plugins, bundled skills, and feature-gated MCP skills converge as commands. Skills use `name/SKILL.md`; dynamic path-dependent project discovery occurs when relevant files are touched. Plugin skills can come from the conventional directory, manifest paths, or marketplace paths. [L1, L2] | Project settings/policy and invocation-time trust gate local discovery. `LoadedFrom` distinguishes managed/bundled/plugin/MCP, but no opaque source-authority/package/resource tuple was found in the inspected skill paths. [L1, A:L1] | Broad frontmatter includes description, allowed-tools, arguments, `when_to_use`, version, model, invocation visibility, hooks, fork/agent/effort, shell, and paths. Only a small name/description/when-to-use estimate is advertised; full loaded content is expanded on invocation. [L1] | Local/plugin/bundled skills are command objects. A focused audit found direct `SKILL.md` reads and a separate experimental remote-search gate, but no authority-bound list/read/search API for those local sources. MCP fetch callsites are visible, but their required loader source is absent; exact MCP URI/catalog semantics are **unknown**. [A:L4, L3, A:L2] | Users invoke commands; the model uses `SkillTool` unless model invocation is disabled. Inline and forked contexts exist. Already-loaded Markdown content is used, unlike Pi/OMP invocation-time reread. Invoked content is tracked for compaction. [L1, L4] |

### Progressive disclosure consequence

All four systems avoid placing every full skill body in the ordinary prompt,
but the mechanisms are not equivalent:

- Pi and OMP disclose metadata, then expose a current filesystem body; explicit
  invocation rereads it.
- Codex exposes bounded metadata and routes a main resource through a provider;
  the host catalog snapshot is immutable metadata, while host content is still
  read from the backing path at read time.
- Claude loads full local/plugin Markdown into command objects but advertises a
  small summary. Invocation expands the loaded content; invoked content is then
  copied into bounded compaction attachments.

Progressive disclosure is a context-budget optimization. It is not a trust
decision, content pin, or permission check.

## 5. Instruction packages versus executable extensions

| System | Instruction/resources/scripts package | Executable module/hook surface | Authority consequence |
| --- | --- | --- | --- |
| **Pi** | A `SKILL.md` package may contain referenced assets, resources, and scripts. The inspected skill loader only parses/renders/reads; no shell/process executor is present in that exact file. [P1, A:P1] | JS/TS extensions loaded with `jiti` run in the CLI process and can register tools, commands, providers, resources, UI, and event handlers. Repository examples exercise tool/command registration, lifecycle/tool hooks, dynamic resources, compaction overrides, UI, subprocess-oriented tools, and same-name built-in tool replacement. Pi documents no extension sandbox. [P2] | Skill prose has only prompt influence; an extension has the host process's code authority. They must not share one trust label. |
| **OMP** | Skills are capability records and local resources. Managed skills are written under an isolated directory by explicit tools/autolearn; they still remain instruction files. [O1, O3] | Built-in and JS/TS extension/hook surfaces can register tools/providers or intercept lifecycle/tool events in-process. Hook wrappers can block or alter results; code remains process-authority code. [O4] | “Built-in”, “managed”, and “autolearned” identify origin/lifecycle, not a permission grant. Executable hooks require a stronger install/admin boundary than skills. |
| **Codex** | Skills are provider-owned instruction/resources. Host/executor/orchestrator reads preserve authority. A referenced script can execute only if another tool/executor is called. [C1] | The inspected app-server uses statically installed, typed Rust extension contributors for goals, guardian, memories, MCP, web, image generation, skills, tools, and lifecycle hooks. No dynamic module loader was found in that exact registry scope. [C4, A:C2] | Typed compiled extensions are trusted host composition, not user skill content. Orchestrator resources retain external authority without giving them host-code authority. |
| **Claude Code** | Local/plugin/bundled/MCP skills are command-like instruction packages. Local/plugin content can contain inline shell expansions, which the host executes through `executeShellCommandsInPrompt` with a tool permission context; MCP content explicitly does not get this expansion. [L1, L2] | Plugins can add skills, commands, agents, hooks, MCP/LSP configuration, and other components. Hook/command execution is executable host behavior, not Markdown authority. [L2] | `allowed-tools` affects a host permission path for inline expansion; it must not be copied as “frontmatter grants tools.” Rollshot should require its own grant for each downstream action. |

Claude's inline-shell behavior is a useful warning. The Markdown does not
execute by magic: a host function interprets shell syntax and constructs a
permission context. Rollshot must never implement an equivalent parser as an
unreviewed shortcut. If Rollshot later supports package scripts, the model or
workflow emits a normal typed tool request; the existing policy/executor decides
whether and where it runs.

## 6. Four distinct authority states

These states must be represented separately even if a UI compresses them into
one sentence.

| State | Example record | What it permits | What it does **not** permit |
| --- | --- | --- | --- |
| **Available** | `CatalogEntry(authority, package, metadata, digest?)` | Show or select bounded metadata. | Reading arbitrary resources, invoking tools, or mutating a document. |
| **Requested capability** | `SkillRequest(package, explicit/implicit, args, turn_id)` | Ask Rollshot to resolve and inject an instruction/resource. | Consent, filesystem/network access, subprocesses, image pixels, or persistent state changes. |
| **Granted permission** | `Grant(subject, action, resource_scope, expiry, run_id)` | Authorize the named product action within scope. | Authority outside its scope/lifetime or authority inherited from skill text. |
| **Execution authority** | `ToolExecutor(environment, policy, cancellation, budget)` | Perform an approved operation and report a typed result. | Broadening its own grant or trusting returned prose as product state. |

The flow should be explicit:

```text
discover metadata
      │
      ▼
available catalog ── explicit/implicit request ──► resolve bounded content
                                                     │
                                                     ▼
                                           model proposes tool call
                                                     │
                                   Rollshot policy + user/product grant
                                                     │
                                                     ▼
                                          typed executor performs work
                                                     │
                                                     ▼
                                      validate → review → product commit
```

A denied grant leaves the skill available. A disabled skill removes or hides
availability but does not revoke unrelated product permissions. A canceled run
invalidates run-scoped grants and executor work; it does not edit the installed
package.

## 7. Identity, path containment, snapshots, and staleness

| System | Resource identity and containment | Snapshot/version semantics | Staleness behavior |
| --- | --- | --- | --- |
| **Pi** | Local file paths; `realpath` canonicalization is used for duplicate identity. No opaque source authority or explicit resource-containment contract was found in the exact skill path. [P1, A:P1] | Catalog metadata is an in-memory load result; explicit invocation rereads the current file. No skill revision/content digest was found in the inspected path. [P3, A:P1] | A file can change between catalog render and explicit invocation. Name-first precedence can also hide later duplicates. |
| **OMP** | `skill://name/path` rejects absolute and lexical parent traversal, resolves under the skill directory, and checks lexical containment. It does not realpath/lstat/readlink the target in the resolver. Therefore an in-tree symlink escape appears possible by source inspection; this is an **inference, not a tested exploit**. [O2, A:O1] | `activeSkills` freezes selected entries for a top-level session, but explicit invocation rereads the path. No authority/revision/content digest was found in the exact skill structures. [O1, A:O1] | Selection may remain stable while file content changes. Managed write hardening does not make reads version-pinned. |
| **Codex** | Opaque authority/package/resource IDs route through the same provider. Executor IDs may be environment-bound; orchestrator reads validate package/resource relation and size. Host reads only an exact cataloged main file. [C1] | `HostSkillsSnapshot` is an immutable metadata/filesystem mapping snapshot, but `read_skill_text` reads the backing file at request time. No content digest/revision pin was found in inspected skill/core-skill paths. [C1, A:C1] | Catalog identity can be stable while bytes change. Orchestrator/executor availability is recataloged or environment-checked, but content version pinning was not found. |
| **Claude Code** | Local skills use realpath identity for deduplication. Bundled references are extracted to owner-only nonce directories with no-follow/exclusive creation and traversal rejection. Plugin component validation, however, only `join`s each declared path and checks existence; containment is not established there. The escape risk is a **source inference**. [L1, L2, A:L3] | Local/plugin Markdown is loaded into command objects; a frontmatter `version` field exists, but no content digest or enforcement of that version was found. Invoked content is copied into transcript attachments. [L1, L4, A:L1] | A session can use loaded bytes after the on-disk file changes. Compaction/resume preserves bounded historical bytes, not a verifiable package release. |

### Rollshot implication

If reproducibility matters, a run needs an immutable `SkillUse` record separate
from the installed catalog:

```text
SkillUse {
  authority,
  package_id,
  resource_id,
  content_digest,
  declared_version?,
  resolved_at,
  invocation_kind,
  bounded_content_or_artifact_ref,
}
```

`declared_version` is display metadata; `content_digest` identifies observed
bytes. For a local package, resource resolution must canonicalize the package
root and candidate, reject symlink escape after canonicalization, reject
special files, and apply byte/file/depth limits. For an opaque provider, the
provider must enforce that the resource belongs to the same authority/package;
Rollshot must not convert its ID into an ambient path.

On catalog refresh, a changed digest creates a new available revision. It must
not mutate a running or historical `SkillUse`. On invocation, an unavailable or
mismatched pinned revision produces a typed stale/unavailable result rather
than silently substituting current content.

## 8. Context budgets, compaction, and resume

| System | Metadata/body budget | Compaction preservation | Session resume |
| --- | --- | --- | --- |
| **Pi** | Metadata validation warns above 64 name characters or 1,024 description characters, but the loader still accepts those over-limit values; only a missing description rejects. No rejecting metadata, skill-body, or aggregate catalog-prompt cap was found in the investigated scope, and explicit expansion injects the complete stripped body. [A:P3] | No invoked-skill continuity attachment was found in the focused Pi profile/source scope; this is an inspected-scope negative, not proof that generic messages cannot retain injected text. [A:P2] | Session JSONL retains messages, but no separately pinned skill identity/content contract was established in this focused audit. [A:P2] |
| **OMP** | The general `skill://` reader reports UTF-8 size but the focused general-skill paths contain no rejecting per-skill body or aggregate metadata cap. The managed-skill writer separately caps generated files at 64,000 bytes; that does not bound authored/general reads. [A:O5, O3] | No dedicated invoked-skill content attachment/version record was found in the exact OMP skill/compaction audit scope. [A:O2] | Process-global `activeSkills` is selection state, not a durable content pin. Durable resume semantics for invoked skill bytes remain **not found in inspected scope**. [A:O2] |
| **Codex** | Metadata: 2% of context, max 4,000 tokens; fallback 8,000 chars. Main prompt: 8,000 bytes. [C2] | No invoked-skill identity/content attachment was found in exact compact sources. Generic injected text may still be summarized; that is not equivalent to preserving the package/digest. [A:C3] | No skill authority/package/resource/version/snapshot record was found in exact rollout reconstruction/thread-store sources. [A:C3] |
| **Claude Code** | Catalog advertisement is metadata-only. At compaction, each invoked skill is head-truncated to 5,000 tokens and the aggregate is capped at 25,000 tokens, newest first. [L4] | `invoked_skills` attachments preserve agent-scoped content; least-recent skills fall out under aggregate pressure and truncated content carries a marker. [L4] | Resume explicitly rebuilds invoked-skill state from those attachments so later compactions retain it. This preserves bounded content, path, and name—not a package authority or verified digest. [L4] |

Rollshot should preserve compact summaries and skill provenance independently:

- compaction may include a bounded instruction excerpt for model continuity;
- durable state retains `SkillUse` identity/digest and an artifact reference;
- resume revalidates provider availability and content digest before any new
  resource read;
- an old result carries its original skill digest and project revision, so the
  product can reject stale application without rerunning the model; and
- permission grants are re-evaluated on resume. They are never reconstructed
  from skill text or the fact that a previous turn invoked the skill.

## 9. OMP managed and autolearned skills

OMP has three distinct ideas that should not be collapsed:

1. **Built-in/native skills** are discovered from normal authored roots.
2. **Managed skills** live under `~/.omp/agent/managed-skills`, are always
   discoverable when skills are enabled, and have the lowest priority so any
   authored same-name skill wins.
3. **Autolearn** can create/update managed skills, but its master flag and
   automatic continuation are both default-off. The controller requires a
   top-level substantive turn (default minimum five tool calls), skips abort,
   plan mode, and goal mode, and only starts a private capture turn when
   `autoContinue` is explicitly enabled. [O3, A:O3]

Managed writes use lowercase/sanitized names, a 64,000-byte cap, a per-name
in-process promise chain, root/directory symlink rejection, exclusive create,
and no-follow/link checks on update. The writer explicitly calls the chain
“in-process only” and cross-process races out of scope; the exact lock audit
found no file/advisory/OS/process lock primitive. [O3, A:O6]

For Rollshot, automatic skill mutation would combine model output, durable code
or policy changes, and future implicit prompt influence. It is not an MVP
requirement for any of the three workloads. If investigated later, creation
must be a reviewable proposal with an explicit diff, tests, signer/source,
version, rollback, and no automatic enablement.

## 10. Claude local, plugin, bundled, and MCP status

Claude's paths have materially different evidence and must remain separate:

- **Local/managed/project/additional:** implemented discovery and command
  construction are visible. Project-dependent discovery is policy/trust gated.
- **Plugin:** implemented loaders accept conventional `skills/`, manifest, and
  marketplace skill paths. Skill contents can use plugin variables/options;
  non-sensitive values may enter model-visible content. Plugins can also carry
  executable hooks/configuration, so plugin trust is broader than skill trust.
- **Bundled:** programmatically registered, and bundled resource trees are
  lazily extracted with stronger temporary-directory/file defenses.
- **MCP:** feature-gated callsites, cache invalidation, and `loadedFrom: 'mcp'`
  filtering are visible. The referenced `src/skills/mcpSkills` implementation
  is absent from the pinned Git tree. Exact discovery protocol, URI ownership,
  version semantics, and containment are therefore **unknown**, not inferred
  from Codex MCP behavior. Visible code deliberately skips inline shell
  expansion for MCP skill bodies. [L1, L2, L3, A:L2]

## 11. Failures, cancellation, and privacy

Skill-layer failures should be typed and should occur before model/tool work
where possible:

- `UnavailableAuthority`, `UnknownPackage`, `UnknownResource`;
- `InvalidMetadata`, `CatalogLimitExceeded`, `ResourceTooLarge`;
- `ContainmentViolation`, `SpecialFileRejected`;
- `DigestMismatch` / `StaleRevision`;
- `ContextBudgetExceeded` with an explicit omission marker; and
- `ProviderTimeout` or `ProviderCancelled`.

Failure to load an optional skill must not weaken policy or expand the remaining
catalog. A resource read should not silently fall back from executor or
orchestrator authority to the host filesystem. An omitted skill due to context
budget should remain explicitly invocable if it is otherwise available.

Cancellation belongs to the provider read and downstream executor separately.
Cancellation after instruction resolution but before tool execution consumes no
execution grant. Cancellation during a tool operation uses the existing run
cancel path and produces no implicit retry unless the product policy requests
one.

Privacy boundaries:

- catalog metadata must not contain secrets, full user paths unless required,
  image content, raw Action Guide input, or plugin option secrets;
- remote/orchestrator search/list/read receives only the minimum query and
  resource handle approved for that authority;
- skill provenance and digest may be durable, while raw resource content should
  use the product's artifact retention policy;
- extension hooks must not observe model input, tool arguments/results, images,
  or credentials merely because they are installed; each event surface needs a
  declared data contract and product-owned grant; and
- logs record authority/package/digest and bounded error categories, not skill
  bodies or user image pixels.

## 12. Rollshot design alternatives and MVP boundaries

These are alternatives for later planning, not a final selection.

### Alternative A — static host instruction catalog

Rollshot ships or explicitly installs allowlisted `SKILL.md` packages from
host-owned roots. At run start it validates metadata/resources, canonicalizes
containment, computes a digest, and creates a run-local immutable catalog.
Only instruction/resource packages exist; there is no runtime extension module,
hook API, remote provider, autolearn, or package script execution shortcut.

**MVP boundary A:** one host authority, explicit invocation plus optional
Rollshot-owned deterministic routing, bounded metadata and one main instruction
resource, content digest, no recursive arbitrary resource reads, and no
third-party executable extensions.

This is the smallest design for Smart Redaction and task-profile help in Action
Guide. It does not satisfy deferred cross-environment or remote package handoff.

### Alternative B — authority-bound provider catalog

Define typed Host/Executor/Orchestrator providers with opaque authority,
package, and resource IDs; bounded `list`, `read`, and optionally `search`;
run-local snapshots; environment binding; content digests; and explicit stale
results. Provider reads are cancelable and budgeted. Tool execution remains a
separate existing registry path.

**MVP boundary B:** start with Host plus at most one concrete non-host provider;
only `list/read` are required, `search` may return capability-unavailable; no
provider can register tools/hooks or translate opaque IDs to ambient paths; no
automatic skill creation/update; no implicit remote call without a Rollshot
policy decision.

This better fits deferred multi-stage workflows and clean authority handoff but
adds catalog refresh, provider failure, identity, caching, and resume work that
the first two workloads do not require.

### Alternative C — trusted compiled extensions plus either catalog

Rollshot may later expose a typed extension registry populated only by bundled
or release-installed Rust contributors. Extensions register narrow typed
capabilities at startup; they do not load arbitrary user JS/TS or inherit skill
trust. Hook phases, observed data, timeout, ordering, cancellation, and failure
policy are explicit.

This enables deep integrations but has the largest product/security surface.
It is not required to validate the instruction-package model and is separable
from Alternatives A/B.

### Trade-off summary

| Dimension | A: static host catalog | B: authority providers | C: compiled extensions |
| --- | --- | --- | --- |
| Smart Redaction fit | Direct, low lifecycle cost | Works but broader than needed | Unnecessary for instruction reuse |
| Action Guide fit | Good for reviewed task profiles | Better if environments become external | Useful only for new native integrations |
| Deferred brag/Hyperframes | Limited handoff/remote identity | Strongest package/resource handoff | Useful for trusted deep integrations, not portable skill content |
| Security surface | Filesystem parser/resolver | Resolver plus provider/network/environment boundaries | Host-code execution and hook data exposure |
| Reproducibility | Straightforward digest snapshot | Requires provider content/version contract | Requires binary/release pin plus skill pin |
| Operational cost | Lowest | Medium/high | Highest |

Preliminary fit: Alternative A is sufficient to test instruction reuse in the
first two workloads; Alternative B addresses deferred authority-preserving
handoff; Alternative C answers a different executable-integration problem.
This is a scope observation, **not a recommendation or final selection**.

## 13. Non-goals and measurable acceptance criteria

### Non-goals for the first skills increment

- no marketplace, dependency solver, automatic updates, or package publishing;
- no autolearned/self-modifying skill enabled by default;
- no arbitrary JS/TS/Wasm/native extension loading;
- no skill-defined permission grants or policy overrides;
- no direct script execution from Markdown/frontmatter;
- no durable worker platform, video pipeline, or Hyperframes implementation;
- no generalized semantic search requirement before a bounded catalog works;
- no retrofitting generic provider messages as the source of product truth; and
- no claim that skills replace Rollshot tools, proposal validation, user
  review, artifact storage, or project revision checks.

### Measurable criteria for a future prototype

1. A catalog of 1,000 entries respects fixed file/depth/byte/time limits and
   emits deterministic ordering and an explicit omission count.
2. The model receives no more than the configured metadata budget and no main
   body beyond its configured byte/token cap.
3. Duplicate names resolve deterministically and retain source provenance; a
   collision cannot silently replace a higher-trust package.
4. Absolute paths, lexical `..`, canonical symlink escape, special files, and
   oversized resources all produce typed failures in tests.
5. Every resource read proves same authority/package ownership; opaque executor
   or orchestrator IDs are never passed to host filesystem APIs.
6. Changing a file after run snapshot yields `DigestMismatch`/`StaleRevision`;
   the run never silently consumes new bytes.
7. Explicit and implicit invocation produce distinguishable audit records;
   implicit routing can be disabled without disabling explicit invocation.
8. A skill requesting shell/network/image/document mutation cannot perform it
   without a separately testable Rollshot grant and normal tool call.
9. Cancellation interrupts provider read and script/tool execution within a
   measured deadline, and no canceled result is applied.
10. Compaction preserves bounded instruction continuity plus authority/package/
    digest identity; resume revalidates availability and does not restore old
    permissions from the transcript.
11. Smart Redaction and Action Guide stale-result tests continue to reject a
    result created against a prior product revision even when the skill digest
    is unchanged.
12. Logs and persisted audit records contain no instruction body, plugin option
    secret, raw semantic input, or image pixels in privacy tests.

## 14. Open gaps and focused spikes

1. Benchmark prompt quality for metadata-only routing versus deterministic
   Rollshot preselection on the three workloads; do not begin with embedding
   search as an assumption.
2. Decide whether the first local snapshot stores bounded content bytes or a
   content-addressed artifact reference; test crash/resume and package removal.
3. Runtime-test OMP `skill://` and Claude plugin declared-path symlink/parent
   escape in disposable fixtures before treating the source inferences as
   vulnerabilities. [A:O1, A:L3]
4. If Claude MCP skills materially affect a later decision, obtain an auditable
   source/package containing `mcpSkills` and re-run discovery/authority/version
   analysis. Current semantics remain unknown. [A:L2]
5. Define provider search only after a workload demonstrates list/read is
   insufficient. Codex's typed search contract with empty implementations is
   evidence that interface presence is not implemented capability. [A:C1]
6. Prototype canonical path containment on Linux and macOS, including symlink
   swaps between validation and open; prefer handle-relative/no-follow APIs
   where available.
7. Specify exactly which compiled extension events can observe image/document/
   model/tool data before implementing any hook registry.

## 15. Evidence index

### Shared and Rollshot

- **[E0] Graph-first boundary:** code-review-graph
  `get_minimal_context` returned `7,979 nodes / 65,744 edges / 405 files` for
  Rollshot and `0 nodes / 0 files` for each pinned reference checkout; direct
  bounded source inspection was therefore used for references.
- **[E1] Revision pins:** `git -C <checkout> rev-parse HEAD` for the four source
  trees; the hashes are recorded at the top of this document.
- **[E2] Umbrella and Round 0:**
  `docs/researchs/agent-foundation/README.md` and the **In Progress** Round 0
  baseline `docs/researchs/agent-foundation/00-rollshot-baseline-workloads.md`;
  workload, authority, evidence, and no-selection requirements.
- **[E3] Rollshot source:** `crates/rollshot-agent/src/{domain,driver,model,
  provider,runtime,tools}.rs`; typed tools, authorized model input, budgets,
  cancellation, terminal states, and proposal flow. See also the **In
  Progress** Rollshot Round 0 baseline and capability comparisons for tools,
  budgets, context, and persistence.
- **[A:R0] Exact negative audit:** semantic graph search for
  `skills extension plugin catalog invocation authority snapshot` returned
  zero Rollshot nodes. Then `rg -n -i '(struct|enum|trait) Skill|SKILL.md|
  skill_(catalog|provider|invocation|snapshot)|extension_registry|
  plugin_registry'` over exactly the six files in [E3] returned zero hits.

### Pi

- **[P1] Discovery/model:**
  `learn-projects/pi/packages/coding-agent/src/core/skills.ts`, plus skill
  docs/tests cited by the Reviewed Pi profile; source roots, metadata parsing,
  deduplication, prompt rendering, and package resource convention.
- **[P2] Extension/trust/examples:** Pi `docs/extensions.md`,
  `docs/security.md`, extension loader/runner paths, and bounded inspection of
  `packages/coding-agent/examples/extensions/{README.md,tools.ts,
  dynamic-resources/index.ts,custom-compaction.ts,built-in-tool-renderer.ts}`;
  in-process `jiti` execution, project trust, registrations, and hooks.
- **[P3] Explicit invocation:**
  `learn-projects/pi/packages/coding-agent/src/core/agent-session.ts`,
  `_expandSkillCommand`; current file reread and prompt injection.
- **[A:P1] Exact audit:** case-insensitive `rg` for `allowed-tools`,
  `allowedTools`, skill version/revision/snapshot, authority/package/resource,
  content hash/digest, and containment terms over `skills.ts` and
  `agent-session.ts` returned zero hits. Shell/process executor terms over
  exactly `skills.ts` also returned zero hits.
- **[A:P2] Exact compaction/resume audit:** search for invoked skill, skill
  invocation/authority/package/resource/snapshot/revision/version/content
  digest over exactly `core/compaction`, `core/session-manager.ts`,
  `core/agent-session-runtime.ts`, and `core/agent-session-services.ts`
  returned zero hits. Generic message persistence is not a substitute for that
  missing skill-specific contract.
- **[A:P3] Exact cap audit:** search for max/limit/cap/budget/token/body/
  metadata/size/bytes/truncation/length terms over exactly `core/skills.ts`,
  `core/agent-session.ts`, and `core/resource-loader.ts` found
  `MAX_NAME_LENGTH=64` and `MAX_DESCRIPTION_LENGTH=1024` warning thresholds,
  generic session token accounting, and complete `readFileSync`/frontmatter-
  strip body expansion. Values above those thresholds remain loaded; only a
  missing description rejects the skill. No rejecting metadata, skill-body, or
  aggregate skill-metadata prompt cap was found in those paths.

### oh-my-pi

- **[O1] Capability/discovery/invocation:**
  `packages/coding-agent/src/{capability/skill.ts,extensibility/skills.ts,
  discovery/*.ts}` and explicit skill expansion paths in the pinned OMP tree.
- **[O2] Resource protocol:**
  `packages/coding-agent/src/internal-urls/skill-protocol.ts`; lexical
  validation, resolution, containment check, and resource read.
- **[O3] Managed/autolearn:**
  `packages/coding-agent/src/autolearn/{managed-skills,controller}.ts`,
  `discovery/builtin.ts`, and `config/settings-schema.ts`.
- **[O4] Extensions/hooks:** OMP extension loader, hooks, and documentation
  paths cited by the Reviewed OMP profile.
- **[A:O1] Exact audit:** `rg` for skill authority/package/resource,
  version/revision/digest, `realpath`, `lstat`, and `readlink` over exactly
  `capability/skill.ts`, `extensibility/skills.ts`, and `skill-protocol.ts`
  returned only `realpath` uses in `extensibility/skills.ts` for deduplication;
  the protocol resolver had no `realpath`/`lstat`/`readlink` hit.
- **[A:O2] Exact compaction/resume audit:** search for invoked skill, skill
  invocation/authority/package/resource/snapshot/revision/version/content
  digest over exactly `session/compact-modes.ts`, `snapcompact-inline.ts`,
  `session-persistence.ts`, `session-loader.ts`, and `session-entries.ts`
  returned zero hits.
- **[A:O3] Status audit:** `config/settings-schema.ts` declares both
  `autolearn.enabled` and `autolearn.autoContinue` with `default: false` and
  `minToolCalls` default 5; `discovery/builtin.ts` states managed discovery is
  unconditional and lowest-priority while writing/nudging is gated.
- **[A:O4] Exact search-API audit:** search for skill-search/search-skill,
  `SkillSearch`, `search(`, `query(`, `find(`, catalog, and provider over
  exactly `capability/skill.ts`, `extensibility/skills.ts`,
  `internal-urls/skill-protocol.ts`, and the built-in/Claude/Codex/OMP-plugin
  discovery providers found provider-labelled discovery comments/source fields
  and name-based `find`, but no skill search/query/catalog API or
  authority-routed search.
- **[A:O5] Exact cap audit:** search for max/limit/cap/budget/token/body/
  metadata/size/bytes/truncation/validation terms over exactly
  `capability/skill.ts`, `extensibility/skills.ts`, `skill-protocol.ts`, and
  `discovery/builtin.ts` found body construction and a returned byte `size`, but
  no rejecting general per-skill body or aggregate metadata cap. The distinct
  managed writer's `MAX_MANAGED_SKILL_BYTES=64_000` is recorded in [O3].
- **[A:O6] Exact cross-process-lock audit:** search for flock, file/process/
  advisory/OS lock, lockfile, mutex, semaphore, `withLock`, `O_NOFOLLOW`,
  `O_EXCL`, `wx`, and link-count terms over exactly `autolearn/managed-skills.ts`,
  `autolearn/controller.ts`, `tools/learn.ts`, and `tools/manage-skill.ts` found
  no cross-process lock primitive. Positive evidence is the
  `skillMutationChains` in-memory `Map`/Promise chain and its explicit
  “in-process only; cross-process races are out of scope” comment; filesystem
  hits were `O_NOFOLLOW`, `nlink`, and exclusive `wx` create defenses.

### Codex

- **[C1] Provider/identity:**
  `learn-projects/codex/codex-rs/ext/skills/src/{catalog,provider}.rs`,
  `provider/{host,executor,orchestrator}.rs`, and
  `core-skills/src/{service,loader,model}.rs`; authority-preserving list/read/
  search contract and provider-specific catalogs.
- **[C2] Context bounds:** `ext/skills/src/render.rs` constants and rendering;
  `DEFAULT_SKILL_METADATA_CHAR_BUDGET=8000`, max metadata 4,000 tokens at 2%
  of context, and main prompt 8,000 bytes.
- **[C3] Invocation/tools:** `ext/skills/src/{extension,
  shadow_selection_experiment}.rs`, `core-skills/src/{invocation_utils,
  model}.rs`, `core/src/skills.rs`, and `tools/{list,read}.rs`; explicit
  selection, behavior-based implicit attribution, shadow selector metrics,
  model tool scope, and recatalog/read.
- **[C4] Executable extensions:**
  `codex-rs/app-server/src/extensions.rs` and
  `ext/extension-api/src/registry.rs`; statically composed typed contributors.
- **[A:C1] Exact audits:** all `SkillProvider::search` implementations in host,
  executor, and orchestrator return `SkillSearchResult::default()`; the tools
  directory contains only `list.rs`, `read.rs`, `schema.rs`, and `mod.rs`, with
  no `search.rs`. Search for skill revision/version/content hash/digest in
  `ext/skills/src` plus `core-skills/src` returned zero hits.
- **[A:C2] Exact negative audit:** search for `dlopen`, `libloading`, `jiti`,
  dynamic/module/plugin loading over exactly `ext/extension-api/src` and
  `app-server/src/extensions.rs` returned zero hits. This does not claim the
  whole Codex repository has no plugin/package loader.
- **[A:C3] Exact persistence/compaction audit:** search for invoked skill,
  skill invocation/authority/package/resource/snapshot/revision/version over
  exactly `core/src/compact*.rs`, `session/rollout_reconstruction.rs`, and
  `thread-store/src` returned zero hits.

### Claude Code

- **[L1] Local skill loader/tool:**
  `learn-projects/claude-code-source-code/src/skills/loadSkillsDir.ts`,
  `src/tools/SkillTool/SkillTool.ts`, `src/utils/frontmatterParser.ts`, and
  `src/utils/promptShellExecution.ts`; roots, metadata, disclosure, invocation,
  trust, and inline-shell policy path.
- **[L2] Plugin/bundled:**
  `src/utils/plugins/{pluginLoader,loadPluginCommands}.ts` and
  `src/skills/bundledSkills.ts`; plugin component paths/content and secure
  bundled resource extraction.
- **[L3] MCP callsites:**
  `src/services/mcp/{client,useManageMCPConnections}.ts`, MCP utilities, and
  `SkillTool`; feature gate, fetch/cache callsites, and `loadedFrom: 'mcp'`.
- **[L4] Compaction/resume:**
  `src/services/compact/compact.ts`, `postCompactCleanup.ts`,
  `src/bootstrap/state.ts`, and `src/utils/conversationRecovery.ts`;
  `POST_COMPACT_MAX_TOKENS_PER_SKILL=5000`, aggregate 25,000, attachment
  creation, retained state, and resume reconstruction.
- **[A:L1] Exact audit:** search for content hash/digest, skill revision,
  pinned version, and authority/package/resource types over exactly local skill,
  `SkillTool`, plugin command, compact, and recovery paths returned zero hits.
  This does not erase the parsed, descriptive frontmatter `version` field.
- **[A:L2] Hidden-source audit:** `git ls-tree -r --name-only` at the pinned
  Claude revision found no `src/skills/mcpSkills.{ts,tsx,js}` while visible MCP
  callsites require `../../skills/mcpSkills.js`. MCP implementation semantics
  are therefore unknown in this source snapshot.
- **[A:L3] Plugin containment audit:** `validatePluginPaths` in
  `src/utils/plugins/pluginLoader.ts` maps each configured relative path through
  `join(pluginPath, relPath)` and `pathExists`; it does not canonicalize and
  compare that component path with the plugin root. Other `realpath` uses exist
  elsewhere in the loader and are not claimed to cover this function.
- **[A:L4] Exact local list/read/search API audit:** search for provider,
  catalog, authority/package/resource ID, skill-list/read/search, `search(`,
  and `query(` terms over exactly `skills/loadSkillsDir.ts`,
  `skills/bundledSkills.ts`, `utils/plugins/loadPluginCommands.ts`, and
  `tools/SkillTool/SkillTool.ts` found a direct plugin `SKILL.md` read and
  feature-gated `EXPERIMENTAL_SKILL_SEARCH` remote-module references. It found
  no authority-bound list/read/search API for local/plugin/bundled commands.
  The experimental remote search is a separate source path and is not evidence
  for the missing MCP loader in [A:L2].

## 16. Limitations

- This is static source analysis. The two stated path-escape concerns are
  source inferences pending disposable runtime tests.
- Reference repositories were outside graph coverage; the recorded graph-zero
  boundary explains the bounded direct inspection but does not improve it.
- Claude MCP skill implementation is absent from the pinned source snapshot;
  only callsite/status claims are made.
- Pi and OMP skill-specific compaction/resume audits were narrower than their
  general session implementations. Negative claims are limited to dedicated
  invoked-skill identity/content/version preservation, not generic retention of
  already-injected messages.
- Codex has a typed search contract and selector experiments, but inspected
  provider search methods are empty and candidate selection is shadow-only at
  this revision. Behavior-based implicit invocation attribution is implemented;
  neither interface nor measured selector experiment is production search.
- Frontmatter and documentation can describe intended behavior; source paths
  above were used for load, trust, invocation, authority, and lifecycle claims.
- No runtime performance, usability, or prompt-quality comparison was executed.
- This document deliberately does not choose a final Rollshot design.
