# Spikes

Throwaway feasibility experiments for the Smart Redaction Agent Workbench
(see `docs/superpowers/plans/2026-06-20-smart-redaction-spikes.md`).

- Each subdirectory is a **standalone** Rust crate with an empty `[workspace]`
  table. None are members of the root workspace and none are built by `cargo`
  from the repo root.
- Each crate's primary output is its `FINDINGS.md`.
- After a decision is consumed these become `retained-reference`: historical
  evidence only. Do not import them from production code, keep them synced, or
  delete them without an explicit request. Committed `Cargo.lock` files are
  frozen decision-time evidence — do not re-resolve them later.
