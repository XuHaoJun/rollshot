# Issues

File-based issue tracker for rollshot. Each issue is a single markdown
file in this directory.

To file a new issue, copy [ISSUE_TEMPLATE.md](./ISSUE_TEMPLATE.md) and
fill it in.

## Conventions

### Filename

`YYYY-MM-DD-kebab-case-slug.md`

The date is the filing date and never changes — it's an identifier, not a
status. Example: `2026-05-22-capture-stitcher-perf-regression.md`.

### Front matter

YAML front matter at the top of the file holds machine-readable metadata.

| Field      | Required | Notes                                                       |
| ---------- | -------- | ----------------------------------------------------------- |
| `title`    | yes      | Same as the `# H1` below — duplicated so tooling can read it without parsing markdown. |
| `status`   | yes      | One of: `open`, `in-progress`, `resolved`, `closed`.        |
| `date`     | yes      | ISO 8601 (`YYYY-MM-DD`). Matches the filename prefix.       |
| `severity` | no       | `low` \| `medium` \| `high` \| `critical`. Bugs only.       |
| `reporter` | no       | GitHub handle or name.                                      |
| `tags`     | no       | List of short kebab-case labels, e.g. `[capture, macos]`.   |

### Status lifecycle

```
open ──► in-progress ──► resolved ──► closed
  │                          ▲
  └──────────────────────────┘   (skip in-progress for trivial issues)
```

- **open** — filed, not yet started.
- **in-progress** — someone is actively working on it.
- **resolved** — fix is merged but kept around for reference / verification.
- **closed** — confirmed fixed, or dropped (won't-fix). Add a `## Resolution`
  section explaining why.

Update `status` in place. Do **not** rename the file when status changes —
the date prefix is an identifier, not a status field.

### Closing an issue

Append a `## Resolution` section at the bottom of the file noting:
- The fix commit / PR (short SHA + title, or `#NN`).
- Anything a future reader should know (regression risk, follow-ups).

Then flip `status: closed`.

### Discoverability

- `ls docs/issues/` — chronological listing (date-prefixed filenames sort
  naturally).
- `rg '^status: open' docs/issues/` — find all open issues.
- `rg '^tags:.*capture' docs/issues/` — find issues by tag.

## Why file-based?

- Survives GitHub outages and repo migrations.
- Reviewable in PRs alongside the code that fixes them.
- Greppable, diffable, and works offline.
- No extra tool to install — any editor will do.

GitHub issues remain the right place for outside contributors and
discussion threads. This directory is for issues the rollshot team owns
and wants tracked in-tree.
