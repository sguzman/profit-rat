# Profit Rat Bootstrap Roadmap

## Current Status
- Rule: check a box only when code, tests, and docs for that item are done.
- Progress: bootstrap foundation and initial feature slices were implemented in this pass.
- Validation blocker: `cargo check` could not complete on 2026-06-17 because `crates.io` fetches for missing dependencies timed out, so milestone boxes stay open until the build and tests can be run successfully.

## Milestones
- [ ] Milestone 0: Create the project skeleton, additive ignore rules, `.cache`-first config, startup logging, `/ping`, and a healthy boot path.
- [ ] Milestone 1: Add user bootstrap, integer fake-money balances, hourly claim handling, and a balance event ledger.
- [ ] Milestone 2: Add native market creation, listing, market detail views, and native resolution flow.
- [ ] Milestone 3: Add native `/buy`, `/sell`, `/positions`, LMSR pricing, and transactional writes.
- [ ] Milestone 4: Add leaderboard and operator-facing ergonomics with strong logging and error reporting.
- [ ] Milestone 5: Add Manifold tracking, cached external snapshots, market-type-aware trading, and sync-based settlement.

## Deliverables
- [x] Create the initial crate layout around `bot`, `config`, `db`, `domain`, `services`, and `integrations`.
- [x] Store runtime state, database files, and logs under `.cache`.
- [x] Add SQLx migrations for users, balances, markets, positions, trades, and external snapshots.
- [x] Add extensive structured logging around startup, commands, database work, pricing, and settlement.
- [x] Add startup and domain tests for config defaults, cache directory creation, pricing monotonicity, claims, and migrations.
- [x] Add Discord slash commands for bootstrap, native market flow, and Manifold tracking flow.
