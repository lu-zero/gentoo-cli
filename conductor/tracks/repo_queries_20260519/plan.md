# Plan: Repo Query Commands and Regen

## Phase 1 — Read-only repo queries

- [x] Task: implement `em query list [pattern]`
    - [x] Add `src/query/list.rs` — walk repo ebuilds, collect CPVs, filter by pattern
    - [x] Wire into `run_query` in `main.rs`
    - [x] Commit: `feat: implement query list`

- [x] Task: implement `em query which <atom>`
    - [x] Add `src/query/which.rs` — find best-matching ebuild path for atom
    - [x] Wire into `run_query`
    - [x] Commit: `feat: implement query which`

- [x] Task: implement `em query keywords <atom>`
    - [x] Add `src/query/keywords.rs` — collect keywords from all versions, render table
    - [x] Wire into `run_query`
    - [x] Commit: `feat: implement query keywords`

- [x] Task: implement `em query uses <atom>`
    - [x] Add `src/query/uses.rs` — read IUSE from best-matching cache entry
    - [x] Wire into `run_query`
    - [x] Commit: `feat: implement query uses`

- [x] Task: implement `em search <pattern> [--description]`
    - [x] Add `src/search.rs` — filter CPNs (and optionally DESCRIPTIONs) by pattern
    - [x] Wire into `run_applet`
    - [x] Commit: `feat: implement search`

- [x] Task: implement `em query hasuse <flag>`
    - [x] Add `src/query/hasuse.rs` — walk all packages, filter by IUSE membership
    - [x] Wire into `run_query`
    - [x] Commit: `feat: implement query hasuse`

- [ ] Task: Conductor - User Manual Verification 'Phase 1' (Protocol in workflow.md)

## Phase 2 — Solver-powered reverse deps

- [x] Task: implement `em query depends <atom>`
    - [x] Add `src/query/depends.rs` — load full dep graph, collect reverse RDEPEND/DEPEND edges
    - [x] Wire into `run_query`
    - [x] Commit: `feat: implement query depends`

- [ ] Task: Conductor - User Manual Verification 'Phase 2' (Protocol in workflow.md)

## Phase 3 — Metadata cache regeneration

- [x] Task: implement `em regen`
    - [x] Add `src/regen.rs` — async regen using `portage-repo` regen_cache, `-j`, `--dedup`, `--output`
    - [x] Wire into `run_applet`
    - [x] Commit: `feat: implement regen`

- [ ] Task: Conductor - User Manual Verification 'Phase 3' (Protocol in workflow.md)
