# Permissions and sandboxing comparison

**Research date:** 2026-07-22 (Asia/Taipei)  
**Status:** Reviewed  
**Umbrella revision:** 1  
**Current Rollshot revision:** `4bb11350a9b54638cc623db316885db58595a47a`  
**Reference revisions:** Pi `dd6bea41efa8caa7a10fe5a6401676dc5699f83f`;
oh-my-pi `7b141199d524b859c357fc89654f10b62b9f3df1`; Codex
`4a443994bd12f49f2f08b21a2f224d9d42b9e734`; Claude Code source
`2ca5ddabfed5f220812ea11f029eda03b21bc4c1`.  
**Evidence mode:** static source, test-source, and repository-documentation
inspection. Rollshot and reference runtime permission dialogs, operating-system
sandboxes, disconnect races, revocation, and crash/restart behavior were not
executed. Reference knowledge-graph queries returned zero nodes and edges, so
the exact pinned trees were inspected with bounded source searches. [E:U0]

This document compares authority, approval, sandbox, trust, persistence, and
audit models. It does **not** select a final Rollshot architecture.

## 1. Problem and workload traces

Rollshot must authorize product capabilities, not merely commands. A coding
agent can often treat `bash` as the authority concentrator; Rollshot cannot.
Screen pixels, listen-only input events, model credentials, local files, and a
publish destination have different disclosure, lifetime, and revocation rules.

| Workload | Authority trace | Required boundary |
|---|---|---|
| **Smart Redaction** | Product already owns a screenshot. The user chooses an OCR/layout-only or full-screenshot payload; the workbench constructs bounded `AuthorizedModelInput`; current Smart Redaction requests still send no image attachment to the provider. Inspection and proposal Tools operate inside a bounded run; application occurs only through review. [E:R1] | Product disclosure consent must remain distinct from Tool availability and provider credential use. No agent prompt may silently widen OCR-only to full pixels. Review is authorization to apply a proposal, not retrospective authorization to disclose pixels. |
| **Action Guide** | Capture backend obtains screen frames. A separate platform source may observe privacy-filtered semantic input. macOS requests Input Monitoring for a listen-only event tap, never Accessibility/PostEvent; Linux opens readable evdev devices. Failure degrades to visual-only. Export writes a user/product-selected destination. [E:R2, E:R3] | Screen Capture and Input Monitoring/device access are separate grants. Input is start/stop scoped, listen-only, and raw key codes, typed text, device names, and paths must never enter the public model. Export is a fresh filesystem/publish decision. |
| **Deferred brag + Hyperframes** | A future pipeline may inspect project files, invoke renderers, use model/provider credentials, make network calls, retain intermediate media, and publish final artifacts. Some work may outlive a foreground turn or move to a remote worker. [E:R0, E:H0] | Every durable Job must reattach current authority, use least-privilege credential handles, distinguish spill from Product Artifact, and require explicit publish authority. This workload is evidence for requirements, not proof Rollshot needs a general remote executor. |

The central design question is therefore: **which Rollshot-owned authority is
available, requested, granted, enforced, and audited for one concrete
operation, and what survives when execution changes owner or lifetime?**

## 2. Terms and non-equivalent boundaries

### 2.1 Five-stage authority lifecycle

```text
capability implementation exists
        -> available for this Run/Task/Job
        -> concrete request (resource + action + reason)
        -> grant (scope + duration + grantee + constraints)
        -> execution under enforcement
        -> privacy-safe audit receipt
```

| Stage | Meaning | Must not be inferred |
|---|---|---|
| **Availability** | A Tool/backend/provider is healthy, configured, and exposed in this context. | User consent, OS permission, credential access, or authority to execute. **Availability is not authority.** |
| **Request** | A concrete operation names requested resources, actions, destination, reason, and expected effects. | Approval or a future reusable grant. |
| **Grant** | A product/user/policy decision binds authority to a grantee, scope, duration, and constraints. | Successful enforcement, persistence after resume, or authority for a child. |
| **Execution** | An executor uses a grant while the sandbox/broker/OS enforces it. | That the effect completed, is retry-safe, or remains authorized. |
| **Audit** | A privacy-minimal receipt records decision and effect metadata. | Permission to retain raw input, secrets, command output, or pixels. |

### 2.2 Trust, approval, sandbox, and operating-system permission

| Boundary | Question answered | Example | Non-equivalence |
|---|---|---|---|
| **Project trust** | May repository-controlled configuration or executable extensions load? | Pi's nearest canonical-directory trust decision. [E:P1] | Not a command approval and not process isolation. Once trusted, Pi still has the user's ambient OS authority. |
| **Approval policy** | Must this proposed Tool/command ask, deny, or proceed? | OMP Tool tiers; Codex `AskForApproval`; Claude allow/deny/ask rules. [E:O1, E:C1, E:L1] | A policy decision is not enforcement if the executor retains broader ambient access. |
| **Sandbox** | What can the executing process actually read, write, or reach? | Codex `PermissionProfile`; Claude's optional sandbox runtime. [E:C2, E:L2] | A sandbox does not grant Screen Capture, input listening, a credential, or publishing intent. |
| **OS permission** | Has the platform granted a protected service? | macOS Screen Recording or Input Monitoring. [E:R2] | It may be broad and persistent at OS level; Rollshot still needs a narrower per-operation product grant. |
| **Credential lease** | May this grantee use a named secret/service for a bounded purpose? | Proposed opaque provider credential handle. | An environment variable or readable config file is not a typed lease. |
| **Review/publish consent** | May a proposal mutate a Product document or cross an irreversible boundary? | Apply Smart Redaction proposal; export/share a guide. [E:R1, E:R3] | Not equivalent to permitting generation, file write, or network egress generally. |

### 2.3 Scope, cache, escalation, and revocation

A grant needs at least `{authority_kind, resource, actions, grantee, scope,
issued_at, expires_at, policy_revision, provenance}`. Scope may be one
invocation, attempt, Turn, Agent Run, Product Task, Job lease, or Session.
“Always allow” is a policy rule, not an immortal grant.

An approval cache is safe only when its key includes the normalized resource,
action, effective constraints, executor/environment identity, and policy
revision. Duration and invalidation must be explicit. Escalation is a new
request for a strict superset; a denied read path must never be smuggled into an
unsandboxed retry. Revocation stops future admission immediately and should
signal live operations when the platform permits, while recording that an
already-completed external effect cannot be undone.

## 3. Authority inventory

| Authority | Resource/action that must be explicit | Enforcement and audit minimum |
|---|---|---|
| **Filesystem** | Read, create, overwrite, append, delete, rename, enumerate, execute; canonical roots plus deny carve-outs; artifact/export destination. | Descriptor-based or brokered resolution where practical; no-follow/realpath containment; protected control files; byte limits. Audit normalized resource class and effect, not file contents. |
| **Process** | Executable identity, argv class, cwd, environment keys, stdin class, child/background/detached permission, signal/terminate. | Sandbox or broker; clean environment; process tree ownership; bounded output/spill. Audit executable, effect class, exit/termination, not secrets in argv/env. |
| **Network** | Egress protocol/domain/port, local binding, redirects/DNS rebinding policy, remote environment, upload/download limits. | Kernel/proxy enforcement and destination revalidation. Audit policy-class destination and byte counts; redact URLs/query strings. |
| **Credential** | Named provider/account/secret purpose, permitted endpoint/tool, usage ceiling, delegation, expiry. | Opaque handle resolved only inside trusted adapter; never inject all host secrets. Audit handle ID/provider class and use result, never secret material. |
| **Screen Capture** | Display/window/region, still vs stream, pixel disclosure target, start/stop, frame/byte limits. | OS permission plus Rollshot capture session token; visible indicator for streaming. Audit region class, duration/frame count, and recipient, not pixels. |
| **Input events** | Listen-only semantic categories, region/session, start/stop; injection is a separate authority and out of current scope. | OS Input Monitoring/device ACL plus Rollshot source lifecycle; raw-event minimization before persistence/model access. Audit capability/degradation and duration, never raw keys or typed text. |
| **Publish/export** | Local destination or remote service/audience, overwrite policy, artifact identity/revision, irreversible flag. | Product-owned final gate; atomic/no-replace writes where possible; remote idempotency key. Audit artifact digest/revision and destination class, not content or tokens. |

## 4. Current Rollshot behavior and gaps

### 4.1 Agent-core availability is not general authority

`ToolRegistry` holds typed Tool implementations, rejects duplicate names,
advertises their input schemas, enforces argument/result/call limits, checks
cancellation, and executes a model-returned batch serially. Product code builds
different narrow registries for Smart Redaction and visual annotation. This is
a strong **availability** boundary. The inspected six-file agent core has no
separate invocation grant, approval cache, filesystem/network/process sandbox,
credential lease, or generic audit receipt [A:R-AUTH]. `Tool::call` itself does
not receive an authority object. [E:R1]

`AuthorizedModelInput` validates attachment count, dimensions, declared and
actual bytes, media type, and limits; its redacted `Debug` avoids dumping
attachments. Its name records that upstream product code made the disclosure
decision—it does not make that decision. In the current Smart Redaction path,
`PayloadMode::FullScreenshot` builds bytes, but `ModelRequest.attachments` is
still empty; visual annotation is the separate path that forwards attachments.
[E:R1]

### 4.2 Existing restricted-automation enforcement

Rollshot already has an active, purpose-built enforcement layer for generated
Smart Redaction automation. Every `QuickJsExecutor::execute` creates a fresh
`LockedContext`; the rquickjs runtime receives memory and stack ceilings, and
an interrupt handler checks the shared cancellation flag and wall-time
deadline. The context installs only selected ECMAScript intrinsics, explicitly
strips and verifies `eval`, `Function`, `queueMicrotask`, `globalThis`, and
`Reflect`, and does not expose ambient platform globals such as `fetch`,
`require`, `process`, timers, workers, DOM, or browser network APIs. Runtime,
allocation, stack, timeout, evaluation, capability, output, and cancellation
failures remain typed. Fresh-context, lockdown, resource-ceiling, and
in-flight-cancellation tests cover these boundaries. [E:R4, T:R4]

The only installed host API is a frozen `rollshot` object with typed `ocr`,
`layout`, `regionFeatures`, and `templateMatch` callbacks. The validated
artifact's capability manifest supplies the maximum result count per call;
an absent capability or a runtime limit above the manifest is rejected. The
bridge also validates queries/results, truncates even an over-returning host,
charges global and per-capability call ceilings, and charges serialized host
allocation. The execution policy separately caps output bytes and restricts
decoded proposal edit kinds, annotation IDs, candidate count, total affected
area, and bounds. Input and returned host values are deeply frozen. [E:R4,
T:R4]

| Restricted-automation stage | Current enforcement | Boundary it does **not** establish |
|---|---|---|
| **Validated artifact** | Canonical source is revalidated for compatibility; static language/cost rules produce a capability manifest and reject unsupported syntax/imports and configured cost overages. | User consent or authority to acquire the image/capability data [A:R-AUTOMATION-AUTH]. |
| **Fresh language runtime** | New `LockedContext` per execution; dangerous/ambient globals absent; memory, stack, wall-time, cancellation, and typed sandbox errors. | An operating-system sandbox for the Rust host process or arbitrary Tool implementations [A:R-AUTOMATION-AUTH]. |
| **Host bridge** | Only OCR, layout, region-features, and template-match callbacks; manifest result caps, global/per-capability call caps, result validation/truncation, and allocation bounds. | General filesystem, process, network, credential, Screen Capture, input-event, or publish/export authority [A:R-AUTOMATION-AUTH]. |
| **Proposal output** | Byte ceiling, strict decode, allowed edit/annotation sets, and proposal geometry/count/area policy. | Review/apply authorization or permission to publish the proposal [A:R-AUTOMATION-AUTH]. |
| **Active Product wiring** | Existing presets run `QuickJsExecutor` locally through `execute_to_proposal`; the agent workbench builds `RealAutomationHost`, installs `QuickJsExecutor` in `DryRunTool`, uses `smart_redaction_default`, and shares Run cancellation. | A generic Product grant, approval cache, authority receipt, or remote executor lease [A:R-AUTOMATION-AUTH]. |

This is a real sandbox for the restricted JavaScript language/runtime and a
narrow host-capability enforcement seam. Despite the `SandboxError` name, it is
not an OS sandbox or a Product authority broker. The conclusion in Section 4.1
therefore stays narrow: the agent core lacks a generic invocation grant, while
the automation called by one active Tool is already strongly confined.

### 4.3 Product-owned capture, input, and export

macOS streaming capture checks `scap::has_permission()`, may request Screen
Recording, and returns typed `PermissionDenied`; a stable environment flag can
disable prompting. Linux KWin/portal paths preserve or map permission denial.
These are backend/OS gates, not Agent grants. A unified per-capture token,
duration record, or revocation receipt was not found in the focused capture
scope [A:R-CAPTURE]. [E:R2]

Action Guide keeps input authority deliberately narrow. Its platform-neutral
crate owns no native permission API. macOS uses
`CGPreflightListenEventAccess`/`CGRequestListenEventAccess` for the
`kTCCServiceListenEvent` service and a listen-only `CGEventTap`; it explicitly
does not request Accessibility/PostEvent. Linux opens evdev read-only and maps
unreadable devices to permission denial. Both queues are bounded to 4096
privacy-filtered actions; `stop` and `Drop` release observation. Start failure
becomes a persistent visual-only capability instead of blocking capture.
[E:R2]

The Action Guide public model never carries raw key codes, typed text, device
names, or device paths. Export receives a destination from its caller, rejects
an existing directory, cleans a partial folder after failure, and emits
structured tracing. GIF/storyboard paths use temp-sibling plus rename; project
publishing has cancellation and atomic/no-replace machinery. These functions
enforce file-integrity semantics, but no Agent-side publish grant or remote
audience authority was found [A:R-PUBLISH]. [E:R3]

### 4.4 Rollshot gap statement

Current product paths correctly own the sensitive decisions they already make.
The missing abstraction is not “a stronger Tool registry” or “a first
automation sandbox”; it is a typed authority bridge between Product intent/OS
permission and a concrete executor operation. Any future bridge can retain the
existing restricted executor underneath it, but must preserve Smart Redaction
disclosure consent, Action Guide's listen-only semantics,
review-before-apply, and product-owned export. It must not move those decisions
into prompts, model output, or a generic coding-agent permission mode.

## 5. Per-system factual behavior and status

### 5.1 Pi — project resource trust over ambient host authority

At the pinned **Reviewed** revision, Pi intentionally runs with the user's OS
permissions and has no built-in sandbox. Built-in Tools and trusted extensions
can read/write files, spawn processes, use network libraries, and reach
credentials available to the process. Its security guidance recommends an
external container/VM/microVM/remote sandbox with minimal mounts, credentials,
and network for untrusted work. [E:P1]

Pi's built-in boundary is **project resource trust**. It canonicalizes the
directory, applies the closest saved parent/current decision in
`~/.pi/agent/trust.json`, and defaults to ask when trust-requiring resources
exist. Trust controls project settings, packages, themes, prompts, skills, and
extensions; it is explicitly not sandboxing. A session-only decision is
available. Noninteractive ask/never skips project resources; always trusts;
`--approve`/`--no-approve` are one-run overrides. Context files can still load
unless separately disabled. [E:P1, T:P1]

Pi has extension hooks that can block Tool calls, but no built-in per-command
approval/grant cache or managed child/background permission lifecycle was found
in the exact trust, Tool, security, and extension scope [A:P-AUTH,
A:P-LIFECYCLE]. A child-agent example is extension-owned. Session resume
restores conversation/resource state, not a typed authority lease. Specialized
Screen Capture, OS input monitoring/injection, credential-handle, or publish
authority was not found in that focused scope [A:P-SPECIAL].

**Failure stance:** an untrusted project prevents executable project resources
from loading; it does not reduce the authority of Tools that do run. External
sandbox failure behavior belongs to the chosen wrapper, not Pi.

### 5.2 oh-my-pi — approval tiers with ambient execution; optional Task isolation

At the pinned **Reviewed** revision, OMP classifies Tools as read/write/exec and
supports `always-ask`, `write`, and `yolo`, plus per-Tool allow/deny/prompt
overrides. Undeclared Tools default to the conservative exec tier. Normal Tool
execution still uses host authority; the inspected approval documentation
states extensions are unsandboxed. Optional Task isolation changes a Task's
workspace boundary, not the ordinary Tool executor [E:O1, E:O2]. A project
resource-trust boundary comparable to Pi's was not found in the exact approval,
extension-loading, settings, and session scope [A:O-TRUST].

ACP adds a client permission gate for bash and destructive delete/move/edit
intents under the default ACP configuration. Explicit yolo can skip that gate
unless a per-Tool prompt/deny rule applies; ordinary write/edit is not covered
by the ACP destructive-intent gate. `allow_always`/`reject_always` decisions are
cached only in the live `AgentSession`, keyed by Tool and intent, cleared when
the client bridge changes, and are not serialized to JSONL. Disconnect, abort,
cancelled, rejected, and unknown client responses prevent execution—an
important fail-closed boundary. [E:O1, T:O1]

The Task Tool is an exec-tier parent operation. Once approved, its child runs
headless with internal yolo; the parent Task approval is the primary boundary.
Resumed children reconstruct current policy and Tool names rather than restore
a grant. Process-local background Jobs do not establish restart-safe authority.
Specialized Screen Capture, OS input monitoring/injection, credential-handle,
or publish authority was not found in the focused approval/bridge/Task scope
[A:O-SPECIAL].

### 5.3 Codex — orthogonal approval and enforced permission profile

At the pinned **Reviewed** revision, Codex separates `AskForApproval` from
`PermissionProfile`. Approval modes are `UnlessTrusted`, `OnRequest`, granular,
and `Never`; `Never` means no prompt and no user escalation, not automatic
unrestricted execution. Permission profiles are managed filesystem/network,
disabled, or externally enforced. Managed filesystem rules have ordered roots
and deny carve-outs; more-specific paths win and equal-specific denies win.
Protected `.git`, `.agents`, and `.codex` paths and invalid deny globs are
handled fail closed. [E:C1, E:C2]

Shell and patch execution derive approval need, consult session approval
caches, apply optional additional permissions, and attempt execution under the
resolved sandbox. A denied-read path cannot be bypassed through an unsandboxed
retry. `ApprovedForSession` is cached in live session stores; it is not a
durable transcript grant. Approval telemetry records Tool and opaque decision,
not command content. [E:C2, T:C1]

The separate `request_permissions` protocol requests additional filesystem and
network permissions. Responses are intersected with the request and normalized
against the native environment cwd; unknown/foreign environments, cancellation,
disabled granular permission requests, `Never`, empty responses, or strict
auto-review plus Session scope yield no grant. Grants can be Turn or Session
scoped and are held in `TurnState` or live `SessionState`. Both
`request_permissions_tool` and inline exec permission approvals are
**under-development, default-off** features at this revision. They do not cover
credentials, Screen Capture, input events, or publish intent [A:C-SPECIAL].
[E:C1, E:C3, T:C2]

On macOS, Seatbelt enforces the resolved policy. On Linux, split policies use
bubblewrap when legacy Landlock cannot preserve exact semantics; missing
system bubblewrap can use the bundled binary, while unsupported WSL1 sandboxed
commands are rejected. Windows uses supported backends and rejects split
policies it cannot enforce without weakening. An exec-server receives the
native command plus canonical permission context and enforces remotely; the
host intentionally does not wrap that remote command locally. [E:C2]

Children inherit a resolved approval policy, permission profile, environment,
cwd, and conditional exec policy at spawn, but run in separate Threads/Sessions.
Thread resume reconstructs conversation/configuration, not pending approval
futures or process handles. Live exec-server reconnection is narrower: a
bounded 30-second reattach window can retain processes and replay bounded
output. No durable grant restoration should be inferred. [E:C4]

### 5.4 Claude Code — layered rules plus optional OS sandbox

At the pinned **Reviewed** revision, Claude's `ToolPermissionContext` contains
mode, additional working directories, allow/deny/ask rules by source,
bypass/auto availability, and flags for contexts that cannot prompt. Rules can
come from user, project, local, flag, policy, CLI, command, or session sources;
updates explicitly choose a persistence destination. The decision pipeline
checks whole-Tool deny/ask, Tool-specific input checks, content ask and
bypass-immune safety checks, mode, allow rules, and finally converts
passthrough to ask. `dontAsk` converts ask to deny. Permission hooks can decide
and persist suggested updates. [E:L1]

Claude also has an optional OS sandbox adapter backed by
`@anthropic-ai/sandbox-runtime`. Its schema supports network domains, Unix
sockets/local binding, filesystem allow-write/deny-write/deny-read/re-allow,
excluded commands, and policy-only restrictions. It always protects settings
and skill/control paths in its derived filesystem policy. Sandbox enablement is
not the default assumption here: when explicitly enabled but unavailable, the
default is to warn and run commands unsandboxed; `failIfUnavailable: true`
makes startup fail closed. `allowUnsandboxedCommands` defaults true and can be
set false so `dangerouslyDisableSandbox` is ignored. Platform implementation
details below the external sandbox-runtime package were not present in the
pinned source [G:L-SANDBOX-RUNTIME]. [E:L2]

Async agents normally receive an unlinked abort controller and
`shouldAvoidPermissionPrompts`; unresolved asks run PermissionRequest hooks and
then auto-deny. Bubble/in-process contexts can surface prompts. `allowedTools`
replaces inherited session allow rules while preserving explicit CLI rules, so
parent session approvals do not automatically leak through that path. Worker
Tool pools are assembled under the worker's effective mode, not merely copied
from the parent's advertised subset. [E:L3]

Background-agent resume reconstructs transcript/replacement state and current
worker Tool/permission context. It deliberately does not re-run the original
agent-type spawn gate, because the original spawn already passed; subsequent
Tool calls still use current/reconstructed permission checks. This distinction
must not be interpreted as restoring every original grant. Remote permission
prompts can be transported over the bridge, but authoritative remote sandbox,
grant-revocation, and crash-recovery guarantees were not established by static
inspection [G:L-REMOTE]. Specialized Rollshot-like Screen Capture, OS input
event, typed credential lease, or publish grant was not found in the focused
permission/sandbox/Agent scope [A:L-SPECIAL].

## 6. Cross-system authority lifecycle

| System | Availability | Request | Grant/cache | Execution | Audit |
|---|---|---|---|---|---|
| **Rollshot** | Product builds typed per-Run Tool registry; capture/input capabilities report available/degraded. | Product UI chooses payload/capture/input/export; no generic invocation request in agent core or restricted-automation boundary [A:R-AUTH, A:R-AUTOMATION-AUTH]. | OS permission plus product flow; no generic grant/cache [A:R-AUTH, A:R-AUTOMATION-AUTH]. | Typed Tool call; active fresh-context restricted JavaScript with a manifest/policy-bounded vision bridge; platform capture/input; direct export filesystem calls. [E:R4] | Restricted execution emits typed errors/metrics and Product paths use structured tracing, but no unified authority receipt [A:R-AUDIT, A:R-AUTOMATION-AUTH]. |
| **Pi** | Trusted resources plus active Tools/extensions. | Project trust prompt; extension hook may block a Tool. | Nearest canonical-directory persisted trust or session decision; no built-in operation grant/cache [A:P-AUTH]. | Ambient host process; external sandbox only. | Session/Tool events exist; privacy-safe authority receipt not found [A:P-AUDIT]. |
| **OMP** | Enabled Tools and current policy/bridge. | Approval tier; ACP destructive-intent request. | Live per-Tool/intent allow-always cache; cleared on bridge change; not resumed. | Ambient host; optional Task workspace isolation. | Tool/session events; durable authority receipt not found [A:O-AUDIT]. |
| **Codex** | Tool registry, environment, permission profile, feature gates. | Command approval and default-off additional fs/network permission request. | One-shot/session approval cache; Turn/Session additional grants in live state. | Platform or remote enforced permission profile. | Approval telemetry and events; complete durable privacy-minimal authority ledger not found [A:C-AUDIT]. |
| **Claude** | Tool pool, rules/mode, optional sandbox availability. | Tool/input ask, hook/SDK/UI/remote prompt, sandbox override. | Rules persisted to named destinations or session; no general typed resource lease [A:L-GRANT]. | Tool checks plus optional OS sandbox; ambient execution when disabled/unavailable under default warning mode. | Permission denials/events/analytics; durable product-grade authority receipt not found [A:L-AUDIT]. |

## 7. Execution-context and lifecycle matrix

| Context | Rollshot requirement | Pi | OMP | Codex | Claude |
|---|---|---|---|---|---|
| **Foreground** | Ask only with visible, concrete resource/effect; deny on closed prompt channel. | Interactive project trust; no built-in command prompt [A:P-AUTH]. | Tool/ACP prompt with live cache. | Approval plus enforced profile; additional grant feature default-off. | Rule/Tool prompt, hooks/SDK/UI; sandbox orthogonal. |
| **Child agent** | Child receives an attenuated subset bound to child ID and current policy; no ambient parent credential/pixel grant. | Extension-owned child semantics; no core inheritance contract [A:P-LIFECYCLE]. | Parent Task approval, child headless yolo. | Resolved snapshot inherited at spawn into separate Session. | Worker context/rules recomputed; scoped `allowedTools`; async asks auto-deny. |
| **Background** | Requires explicit noninteractive policy or lease; inability to prompt denies; expiry/cancel stops admission. | No managed background authority lifecycle [A:P-LIFECYCLE]. | Process-local Jobs/current policy; restart authority unknown [G:O-RESTART]. | Live background process handles; no durable Job/grant recovery [A:C-JOB]. | Async unlinked controller; hook then auto-deny when prompts unavailable. |
| **Detached/remote** | Environment identity must be part of grant; remote broker enforces; reconnect never widens. | External wrapper is authoritative; core remote grant protocol not found [A:P-REMOTE]. | ACP bridge can gate; remote sandbox/enforcer not established [G:O-REMOTE]. | Exec-server receives canonical permission context and enforces; local host does not wrap remote command. | Bridge transports prompts/status; remote enforcement/revocation remains unverified [G:L-REMOTE]. |
| **Disconnect** | Pending prompts deny; active lease follows declared continue/cancel rule; irreversible effects report unknown if ambiguous. | No managed protocol [A:P-REMOTE]. | Open ACP request rejects/cancels; no silent allow. | Turn cancellation removes pending permission request; live exec-server has bounded reattach. | Remote prompt resumes on response; complete disconnect race behavior not runtime-verified [G:L-REMOTE]. |
| **Resume** | Rebuild availability, then revalidate current policy/OS permission/lease; transcript is evidence, never a grant. | Trust/resource state may reload; no operation grant to restore [A:P-AUTH]. | Policy/Tools reconstructed; live allow-always cache not serialized. | Conversation/config resume; pending approvals and live grants are not durable authority. | Transcript/agent state reconstructed; subsequent Tools use current context; original spawn gate is not repeated. |
| **Restart** | Durable Jobs must reacquire executor lease and current grants; fail closed if authority owner is absent. | External wrapper-dependent [A:P-REMOTE]. | Process-local authority cannot prove restart-safe continuation [G:O-RESTART]. | Exec-server reattach is live/short-window, not crash-durable authority [A:C-JOB]. | Task/output persistence does not prove permission-future or sandbox continuation [G:L-RESTART]. |
| **Revocation** | New calls stop immediately; signal active operations; stop capture/input; revoke credential handle; record partial/irreversible effects. | Saved trust can change future loading; live Tool authority revocation not found [A:P-REVOKE]. | Bridge replacement clears ACP cache; broader live-operation revocation not found [A:O-REVOKE]. | Session caches/live grants can disappear with session/cancel; explicit mid-operation grant revocation not found [A:C-REVOKE]. | Rule/context can change and tasks can be killed; general lease revocation not found [A:L-REVOKE]. |

## 8. Filesystem, process, network, and credential comparison

| Dimension | Pi | OMP | Codex | Claude | Rollshot implication |
|---|---|---|---|---|---|
| **Filesystem** | Ambient OS authority after trust; external sandbox advised. | Approval tier, ambient executor; optional Task workspace isolation. | Managed allow/read/write/deny profile, protected paths, platform enforcement. | Tool path rules plus optional sandbox allow/deny/re-allow. | Separate project read, scratch write, document mutation, and export grants. |
| **Process** | Built-in bash/extensions use host process authority. | Exec-tier approval; children/background are process-local. | Command approval plus sandbox/environment; remote exec-server enforcement. | Bash permission pipeline; optional sandbox; unsandboxed override policy. | Explicit executable/cwd/env/background/signal authority and process-tree ownership. |
| **Network** | Ambient process/network unless external wrapper. | Ambient normal executor; no general network sandbox found [A:O-NETWORK]. | Managed network policy; extra network permission request default-off. | Optional allowed/denied domain and socket/bind controls. | Default deny for agent egress; credential endpoint and redirects constrained together. |
| **Credential** | Config/provider credentials reachable according to process authority; no typed lease [A:P-SPECIAL]. | Credential management exists, but no per-operation authority handle in approval scope [A:O-SPECIAL]. | Specialized typed lease absent from permission profile [A:C-SPECIAL]. | Auth/settings exist, but no typed credential lease in permission/sandbox scope [A:L-SPECIAL]. | Opaque handle resolved inside provider/publisher; no secret values in model, env, log, spill, or Artifact. |

## 9. Screen Capture, input events, and publish/export

Coding-agent shells can sometimes invoke arbitrary capture or publish utilities
under ambient authority. That is precisely why absence of a specialized model
matters: a generic `bash` approval does not express Rollshot's product intent.

| Capability | Pi | OMP | Codex | Claude | Required Rollshot invariant |
|---|---|---|---|---|---|
| **Screen Capture** | Specialized authority not found in focused security/Tool scope [A:P-SPECIAL]. | Specialized authority not found in focused approval/Task scope [A:O-SPECIAL]. | Additional permissions type covers fs/network only [A:C-SPECIAL]. | Specialized authority not found in focused permission/sandbox/Agent scope [A:L-SPECIAL]. | OS permission **and** per-session display/window/region/pixel-recipient token; visible active state; prompt-free denial mode for automation. |
| **Input listen** | Specialized OS event-listen authority not found; extension “Input Events” are TUI input hooks, not OS monitoring [A:P-SPECIAL]. | Specialized authority not found [A:O-SPECIAL]. | Specialized authority not found [A:C-SPECIAL]. | Specialized authority not found [A:L-SPECIAL]. | Listen-only is its own authority; `start -> poll -> stop/Drop`; semantic minimization before queue/model/persistence; visual-only fallback. |
| **Input injection** | Specialized authority not found [A:P-SPECIAL]. | Specialized authority not found [A:O-SPECIAL]. | Specialized authority not found [A:C-SPECIAL]. | Specialized authority not found [A:L-SPECIAL]. | Non-goal for current Action Guide. Never infer injection from Input Monitoring or listen-only source. |
| **Publish/export** | CLI session HTML export exists, but product publish grant not found [A:P-SPECIAL]. | Product publish grant not found [A:O-SPECIAL]. | Product publish grant not found [A:C-SPECIAL]. | Content-specific ask rules can protect commands such as publish, but no typed product publish grant [A:L-SPECIAL]. | Separate final gate names artifact revision, destination/audience, overwrite/irreversible semantics, idempotency key, and receipt. |

## 10. Failure, escalation, cancellation, and retry

### 10.1 Fail-closed rules

- Missing/invalid/expired/mismatched grant denies execution.
- An unavailable approver denies a foreground-only request; background work
  may continue only with an explicit noninteractive policy or still-valid
  lease.
- An unenforceable sandbox profile denies. Claude's default warning-and-run-
  unsandboxed behavior is unsuitable for a Rollshot authority marked required;
  its `failIfUnavailable` option demonstrates the needed distinction. [E:L2]
- A denied read resource cannot be recovered by requesting “run unsandboxed.”
  Codex's denied-read handling is the useful precedent. [E:C2]
- Unknown approval response, disconnect, or cancelled prompt denies. OMP's ACP
  path has source tests for these cases. [T:O1]
- Capture/input failure returns a typed denial/degradation. Input failure falls
  back only to visual-only, never to a broader source. [E:R2]

### 10.2 Escalation

Escalation must compute a minimal diff from current authority, show the exact
resource/effect, and issue a new scoped grant. It must never mutate the base
grant in place. For an executable requesting network and a credential, show
both authorities; approving the network does not approve the credential.
Children may request upward, but only the Product authority owner can issue the
superset. A remote executor cannot self-approve because it reports a failure.

### 10.3 Cancellation, ambiguity, and retries

Cancellation closes pending requests, stops future admission, releases capture
and input resources, revokes handles, and signals owned process trees. A local
write cancelled before atomic rename can report no effect; a network publish
whose acknowledgment was lost is **ambiguous**, not automatically failed.
Retry is permitted only with a fresh/current grant and one of:

1. a proven no-effect attempt;
2. an idempotent operation with the same effect key; or
3. an explicit user decision after showing ambiguity.

Approval is not idempotency. A cached approval must not cause an irreversible
publish to be duplicated.

## 11. Security and privacy edge cases

### 11.1 Paths, symlinks, and TOCTOU

- Normalize for display and policy matching, but enforce using an opened
  descriptor/handle or brokered relative operation. `canonicalize -> check ->
  reopen by path` is vulnerable to replacement races.
- Reject or explicitly resolve symlinks at every traversed component for
  no-follow authorities. Check the final target type. A lexical `starts_with`
  test is insufficient.
- Bind grants to an environment/workspace identity, not just a string path.
  Revalidate after resume, mount change, worktree removal, or remote migration.
- Protect agent-control locations (`.git`, `.codex`, `.claude`, extensions,
  skills, settings, hooks) from self-modification unless separately authorized.
  Codex and Claude both encode variants of this principle. [E:C2, E:L2]
- Use no-replace/atomic commit for exports when supported. Keep destination
  selection product-owned. [E:R3]

Static source review did not establish race-free path enforcement for every
reference Tool/runtime; this remains an execution-level validation gap
[G:PATH-TOCTOU].

### 11.2 Environment and secrets

- Start child processes with an allowlisted environment. Remove provider keys,
  session tokens, proxy credentials, cloud metadata endpoints, SSH agent
  sockets, and unrelated desktop/session secrets unless a typed grant requires
  them.
- Credential values never enter model context, Tool arguments/results,
  command-line arguments, tracing fields, approval descriptions, crash dumps,
  or persisted transcripts. A trusted adapter resolves an opaque handle as
  late as possible and zeroizes/drops it promptly.
- Network policy and credential policy are evaluated together: a credential
  usable only at one provider endpoint must not accompany general egress.

### 11.3 Logs, spills, and Product Artifacts

An inline Tool result, a bounded output **spill**, and a user-visible Product
**Artifact** are separate data classes. A spill is execution-owned, private,
short-lived, access-controlled by the parent operation, and never automatically
attached to model context or published. An Artifact is deliberately promoted,
typed, provenance-linked, retained under Product policy, and reviewable.

Audit receipts contain IDs, authority kind, normalized resource class,
decision source, scope, policy revision, start/end, byte/effect counts, and
terminal status. They exclude pixels, typed input, source contents, full URLs,
raw command output, environment values, and secrets. Debug rendering of
`AuthorizedModelInput` already redacts attachments; this should become a
system-wide invariant. [E:R1]

## 12. Candidate Rollshot patterns and tradeoffs

These are comparison patterns, not a recommendation or final selection.

### Pattern A — product-owned capability snapshot plus managed executor

At Agent Run start, the Product builds an immutable `AuthoritySnapshot` from
current consent, OS state, policy, document revision, environment, and narrow
Tool availability. Each Tool declares required authority and the executor
checks the snapshot or requests an additional grant. Filesystem/process/network
operations run under a managed local sandbox; capture/input/publish remain
special product adapters.

```text
Product intent + OS state -> AuthoritySnapshot -> Tool admission -> sandbox
                                      |                |
                                      `-> prompt ------`-> receipt
```

**Strengths:** fits current per-Run registry and typed state; deterministic;
keeps pixels/input/publish product-owned; can retain the fresh-context
QuickJS executor and narrow vision bridge as an inner enforcement layer while
the Product authority owner/broker governs acquisition and delegation;
relatively small surface.  
**Costs:** snapshots age; mid-run policy/OS revocation needs an epoch check;
background Jobs need a lease layer; the current language sandbox cannot replace
the Product authority broker or an OS sandbox for broader Tools; sandbox
portability work remains.

### Pattern B — live capability broker with short-lived operation tokens

A Product authority broker owns filesystem handles, process launcher, network
proxy, credential handles, capture/input sessions, and publisher. Tools never
receive raw ambient resources. They request an operation token scoped to
resource, action, grantee/environment, attempt, expiry, and policy epoch. Local
and remote executors redeem it through the broker; revocation is centralized.

```text
Tool request -> authority broker -> user/policy/OS -> operation token
                      |                                  |
                      `----------- audit ----------------`-> executor/adapter
```

**Strengths:** strongest attenuation, revocation, remote consistency, and
credential isolation; clean audit point; durable Jobs can reacquire leases.  
**Costs:** substantial broker/handle plumbing; broker availability becomes
critical; filesystem performance and offline work require careful design;
token semantics must resist confused-deputy bugs.

### Pattern C — product-specific gates plus external sandbox boundary

Keep the current narrow registries and explicit Product consent for pixels,
input, review, and export. Retain or extend the current fresh-context QuickJS
and manifest-bounded host bridge, and run its Rust host plus any broader agent
execution inside a separately configured OS/container sandbox with minimal
mounts, environment, and network. Do not add a universal grant object; each
sensitive Product adapter and the Product authority owner/broker still checks
its own typed state.

**Strengths:** smallest change; preserves current product semantics; external
isolation can be independently hardened; reuses proven narrow automation
enforcement.  
**Costs:** fragmented audit/revocation; child/background/remote delegation is
awkward; approval cache semantics remain duplicated; external sandbox policy
can drift from product state. Neither the restricted executor nor the external
sandbox supplies Product disclosure, credential, capture/input, or publish
grants.

### Pattern comparison

| Criterion | Pattern A | Pattern B | Pattern C |
|---|---|---|---|
| Current Smart Redaction fit | Strong | Strong but heavier | Strong |
| Action Guide special permissions | Explicit adapters | Broker-owned sessions | Existing adapters |
| Durable/remote Jobs | Requires added lease service | Native design target | Weak/host-specific |
| Revocation | Epoch checks + signals | Central token/handle revocation | Adapter-specific |
| Audit completeness | Unified per executor | Unified at broker | Distributed |
| Implementation complexity | Medium | High | Low initially |
| Main failure risk | Stale snapshot | Broker/token complexity | Policy fragmentation and ambient gaps |

## 13. Preliminary fit, non-goals, gaps, and measurable criteria

### 13.1 Preliminary fit without selection

All three patterns can preserve Rollshot's current product ownership if they
treat generic coding-agent policies as reference patterns rather than direct
contracts. Pattern A aligns with the current bounded Agent Run; Pattern B
addresses the deferred durable/remote workload more directly; Pattern C is a
credible minimal boundary for present local workloads. Evidence in this round
does not establish enough runtime, platform, or remote data to select among
them.

Pi demonstrates that resource trust and sandboxing must be named separately.
OMP demonstrates a small live approval cache and fail-closed bridge behavior.
Codex demonstrates orthogonal approval/enforcement, path specificity, denied-
read non-escalation, environment-bound grants, and remote permission context.
Claude demonstrates layered rule sources, noninteractive auto-denial, and a
configurable required-vs-best-effort sandbox. None supplies Rollshot's full
Screen Capture/input/publish/credential authority model.

### 13.2 Non-goals

- No final architecture or implementation plan in Round 4.
- No general input injection or macro replay authority.
- No transfer of screenshot, Action Guide, document-review, or publish consent
  from Product code to the model or prompt text.
- No claim that all Tools need a prompt, that all work needs a sandbox, or that
  all grants should persist for a Session.
- No general Workflow/remote-execution system solely to support current Smart
  Redaction.
- No raw event, pixel, source, secret, spill, or command-output content in the
  authority audit ledger.
- No automatic retry of an ambiguous external effect.

### 13.3 Measurable acceptance criteria for a future design

1. Tests cover all seven authority kinds across availability, request, grant,
   execution, audit, denial, expiry, and revocation.
2. A Tool visible to the model but lacking authority cannot execute; a Tool
   unavailable to the model cannot be made available merely by possessing an
   OS permission.
3. Every grant names grantee, normalized resource, actions, scope, expiry,
   policy revision, environment, and provenance; resume/restart never treats
   transcript content as a grant.
4. Symlink-swap and path-replacement adversarial tests cannot escape allowed
   roots; protected control paths remain unwritable; unsupported sandbox
   profiles fail closed.
5. A child never receives authority broader than its parent's delegable set;
   background contexts auto-deny unresolved prompts; remote execution proves
   enforcement of the canonical profile.
6. Revoking capture/input stops future frames/events and releases native
   resources; queues remain bounded; no raw keys, typed text, device identity,
   or pixels appear in logs/audit.
7. Credential tests prove secrets are absent from model messages, Tool JSON,
   argv, environment snapshots, tracing, transcript, spills, and Artifacts.
8. Export/publish requires a fresh artifact-revision/destination grant; no-
   replace and idempotency tests prevent accidental overwrite or duplicate
   remote publication.
9. Disconnect/cancel tests prove pending approval denies and ambiguous effects
   are surfaced; restart tests prove durable work reacquires current authority.
10. Audit receipts correlate request, decision, grant, attempt, effect summary,
    and terminal state without retaining protected payloads.

### 13.4 Open validation gaps

- [G:PATH-TOCTOU] Exercise real symlink swaps, mount changes, rename races, and
  protected-path behavior under candidate local sandboxes on Linux and macOS.
- [G:R-OS] Runtime-test macOS Screen Recording/Input Monitoring prompts,
  revocation while active, Linux portal denial, evdev ACL loss, and Drop/stop.
- [G:C-PLATFORM] Execute Codex split filesystem/network policies on Seatbelt,
  bubblewrap/Landlock, Windows, and exec-server; verify fail-closed behavior.
- [G:L-SANDBOX-RUNTIME] Inspect and execute the exact pinned external
  `@anthropic-ai/sandbox-runtime`; platform internals were outside this source
  tree.
- [G:O-RESTART], [G:L-RESTART], [G:L-REMOTE], [G:O-REMOTE] Exercise
  disconnect, resume, process restart, remote prompt races, current-policy
  changes, and revocation. Static state ownership is not runtime proof.
- Validate approval-cache key collisions, resource normalization, policy epoch
  invalidation, expiry, and cross-environment confusion for any adopted design.

## 14. Evidence index, exact audits, and limitations

### 14.1 Rollshot and workload evidence

- **[E:U0]** Code-review-graph minimal-context queries for Rollshot and all four
  reference roots. Rollshot returned indexed structural context; each ignored
  `learn-projects` root returned zero nodes and zero edges, triggering the
  bounded source fallback required by the repository workflow.
- **[E:R0]** `docs/researchs/agent-foundation/README.md` and
  `00-rollshot-baseline-workloads.md`: umbrella constraints, Round 0 workload
  traces, explicit Product ownership, and deferred workload status.
- **[E:R1]** `crates/rollshot-agent/src/{domain,driver,tools,runtime,model,provider}.rs`;
  `crates/rollshot-app/src/result_workspace/workbench/run.rs`: typed registry,
  budgets/cancellation, attachment validation/redaction, payload construction,
  Smart Redaction empty provider attachments, visual-annotation forwarding.
- **[E:R2]** `crates/rollshot-capture/src/{macos,linux,error}.rs`;
  `crates/rollshot-macos-input/src/{permission,source}.rs`;
  `crates/rollshot-linux-input/src/source.rs`;
  `crates/rollshot-action/src/{lib,input}.rs`; app Action Guide input adapter:
  OS permission mapping, listen-only sources, degradation, queue and privacy
  bounds.
- **[E:R3]** `crates/rollshot-action/src/export/mod.rs`, `gif.rs`,
  `storyboard.rs`, and `project/{store,publish}.rs`: caller-selected destination,
  cleanup, atomic/no-replace operations, cancellation, tracing.
- **[E:R4]** `crates/rollshot-automation-rquickjs/src/{execution,lockdown,bridge}.rs`;
  `crates/rollshot-automation/src/{executor,policy,capability,host,output}.rs`;
  `crates/rollshot-agent/src/tools.rs`; and
  `crates/rollshot-app/src/result_workspace/workbench/run.rs`: fresh restricted
  runtime, manifest/policy-bounded vision bridge, typed failures, proposal
  policy, and active preset/agent dry-run wiring.
- **[T:R4]** `crates/rollshot-automation-rquickjs/tests/{lockdown,resources,end_to_end}.rs`,
  its execution unit tests, and Rollshot automation frontend/output/executor
  contract tests: absent globals, fresh state, compatibility, cancellation,
  memory/stack/time/output/allocation/call/result limits, typed capability
  failures, and edit-proposal policy. Tests were inspected but not rerun for
  this documentation-only correction.
- **[E:H0]** Round 0 deferred brag/Hyperframes workload plus the Round 3/4
  capability documents for durable Job, spill, Artifact, and authority
  separation. This is requirement evidence only.

### 14.2 Reference evidence

- **[E:P1]** Pi `packages/coding-agent/docs/security.md`,
  `src/cli/project-trust.ts`, trust manager, core Tools, and extension docs;
  **[T:P1]** project-trust tests. Profile status: **Reviewed**.
- **[E:O1]** OMP `docs/approval-mode.md`,
  `packages/coding-agent/src/tools/approval.ts`, `session/client-bridge.ts`,
  `modes/acp/acp-client-bridge.ts`, and `agent-session.ts` approval path;
  **[E:O2]** extension-loading and Task isolation sources; **[T:O1]** ACP tests
  for once/always, destructive intents, abort/reject/unknown responses, and
  cache behavior. Profile status: **Reviewed**.
- **[E:C1]** Codex protocol permission/approval types and feature registry;
  **[E:C2]** core sandboxing plus `core/README.md` platform support;
  **[E:C3]** `core/src/session/mod.rs` request normalization and Turn/Session
  storage; **[E:C4]** child/resume/exec-server profile evidence; **[T:C1]**
  exec-policy/sandbox tests; **[T:C2]** request-permission session tests.
  Profile status: **Reviewed**.
- **[E:L1]** Claude `src/Tool.ts`, `src/types/permissions.ts`, and
  `src/utils/permissions/{permissions,PermissionUpdate,permissionsLoader}.ts`;
  **[E:L2]** `src/entrypoints/sandboxTypes.ts`, sandbox adapter, and Bash
  permission/sandbox paths; **[E:L3]** Agent Tool run/resume/context sources.
  Profile status: **Reviewed**.

### 14.3 Exact negative audits

“Not found” below means only “not found in this exact investigated scope at the
pinned revision.” Searches used case-insensitive variants and relevant symbols;
false positives such as TypeScript `export`, UI “input events,” ordinary
credentials, and user-input protocol events were inspected and excluded.

| Audit ID | Exact scope and question | Result |
|---|---|---|
| **[A:R-AUTH]** | Six Rollshot agent-core files for `permission`, `approval`, `sandbox`, `grant`, `authority`, `credential`, `network`, `filesystem`, plus `Tool` call signature. | No generic invocation grant, approval cache, sandbox profile, or credential lease; registry availability and bounded execution are present. |
| **[A:R-AUTOMATION-AUTH]** | `rollshot-automation-rquickjs` execution/lockdown/bridge, `rollshot-automation` policy/capability/host/executor/output contracts and tests, plus active workbench/`DryRunTool` callsites; searched for filesystem/process/network/credential/capture/input/publish plus grant/approval/permission and inspected every installed bridge capability. | Positive narrow enforcement found: fresh restricted JS runtime and OCR/layout/region-features/template-match bridge with resource/output/proposal limits. No user grant, approval/cache, OS sandbox, or general filesystem/process/network/credential/Screen Capture/input-event/publish authority bridge found. |
| **[A:R-CAPTURE]** | Rollshot capture backends/errors for permission request/status, prompt suppression, session/token, expiry/revoke/audit. | OS/backend checks and typed denial found; unified per-capture grant/revocation receipt not found. |
| **[A:R-PUBLISH]** | Action Guide export/project publish and app callsites for authority/grant/approval/audience. | Caller-selected local path, integrity, cancellation, and tracing found; Agent-side publish grant or remote audience authority not found. |
| **[A:R-AUDIT]** | Rollshot agent/capture/input/export diagnostics and models for a correlated authority request→decision→grant→attempt receipt. | Stable structured events and typed errors found; unified receipt not found. |
| **[A:P-AUTH]** | Pi security, project trust, trust manager, built-in Tool implementations, and extension hooks for per-operation grant/approval/cache/sandbox. | Project-resource trust and hook blocking found; built-in operation grant/cache/sandbox not found. |
| **[A:P-LIFECYCLE]** | Pi core/extensions for managed child/background/detached authority inheritance, prompt/disconnect/resume/revocation. | Extension-owned examples only; core lifecycle contract not found. |
| **[A:P-SPECIAL]** | Same Pi scope for OS Screen Capture, OS input monitoring/injection, typed credential handle, and product publish grant. | No specialized authority found; TUI extension Input Events and CLI session HTML export are not these authorities. |
| **[A:P-AUDIT]**, **[A:P-REMOTE]**, **[A:P-REVOKE]** | Pi session/security/trust scope for privacy-safe authority receipt, remote enforcement protocol, and live operation revocation. | Not found; behavior is delegated to an external isolation environment. |
| **[A:O-TRUST]** | OMP approval mode, extension loading, settings/session setup for canonical project-resource trust. | No Pi-like trust gate found; extensions are documented unsandboxed. |
| **[A:O-NETWORK]** | OMP approval/Tool/Task isolation scope for a generally enforced egress profile. | No general network sandbox found. |
| **[A:O-SPECIAL]** | OMP approval, ACP bridge, Task/Job scope for Screen Capture, OS input monitoring/injection, per-operation credential handle, product publish grant. | No specialized authority found. |
| **[A:O-AUDIT]**, **[A:O-REVOKE]** | OMP approval/session/bridge scope for durable privacy-safe authority receipt and general live-operation grant revocation. | Live cache/bridge clearing and events found; durable receipt/general revocation not found. |
| **[A:C-SPECIAL]** | Codex protocol/core permission types and handlers for capture, OS input events, credential lease, and publish grant. | Additional permission profile covers filesystem/network; specialized authorities not found. |
| **[A:C-AUDIT]**, **[A:C-JOB]**, **[A:C-REVOKE]** | Codex approval telemetry, Turn/Session state, background terminal, thread resume, exec-server for durable receipt/Job grants/mid-operation revocation. | Live events/caches/handles found; complete durable authority receipt, crash-durable Job grant, and explicit generic live revocation not found. |
| **[A:L-GRANT]** | Claude permission types/rules/update destinations and sandbox schema for a typed resource lease with grantee/scope/expiry. | Persistent/session policy rules found; general typed lease not found. |
| **[A:L-SPECIAL]** | Claude Tool permission, sandbox, and Agent sources for Screen Capture, OS input monitoring/injection, credential lease, and typed product publish grant. | No specialized authority found; command content rules are not product grants. |
| **[A:L-AUDIT]**, **[A:L-REVOKE]** | Claude permission events/analytics, task kill, rules, and sandbox for durable privacy-safe receipt and generic active lease revocation. | Denials/events/rule changes/task kill found; complete durable receipt/general lease revocation not found. |

### 14.4 Limitations

All findings are pinned snapshots. Static source inspection establishes types,
branches, ownership, and test intent; it does not prove kernel enforcement,
prompt UX, race behavior, remote server behavior, or crash safety. Rollshot's
restricted-automation tests were source-inspected rather than rerun for this
documentation correction, and their language-runtime confinement is not proof
of host-process or kernel isolation. GrowthBook, build-flavor, platform,
managed-policy, and external-package behavior can alter Claude availability.
Codex under-development features are not default product guarantees. OMP and Pi
extensions can implement policies beyond core. No claim about a negative audit
should be generalized outside its listed files and revision.
