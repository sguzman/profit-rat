# Profit Rat Roadmap

## Current Status
- Rule: check a box only when code, tests, and docs for that item are done.
- Progress: bootstrap foundation, native markets, Manifold tracking, and richer Discord UX are implemented and verified.
- Progress: guild-scoped isolation, TOML-first currency policy, direct donations, and share/money loan flows were implemented and re-verified on 2026-06-22.
- Validation: `cargo check` and `cargo test` both pass locally as of 2026-06-22 after the multi-guild and social-economy pass.

## Milestones
- [x] Milestone 0: Create the project skeleton, additive ignore rules, `.cache`-first config, startup logging, `/ping`, and a healthy boot path.
- [x] Milestone 1: Add user bootstrap, integer fake-money balances, hourly claim handling, and a balance event ledger.
- [x] Milestone 2: Add native market creation, listing, market detail views, and native resolution flow.
- [x] Milestone 3: Add native `/buy`, `/sell`, `/positions`, LMSR pricing, and transactional writes.
- [x] Milestone 4: Add leaderboard and operator-facing ergonomics with strong logging and error reporting.
- [x] Milestone 5: Add Manifold tracking, cached external snapshots, market-type-aware trading, and sync-based settlement.
- [x] Milestone 6: Isolate all economy state per guild and filter balances, claims, leaderboards, market lists, positions, and market-holder views by the invoking server.
- [x] Milestone 7: Add TOML-backed rich currency formatting plus direct donation and loan interactions for money and shares.
- [ ] Milestone 8: Replace legacy share-transfer storage with the generalized `asset_offers` model and add acceptance-based money offers.
- [ ] Milestone 9: Expand tests around guild migration edge-cases, donation/loan flows, and overdue/default behavior.

## Deliverables
- [x] Create the initial crate layout around `bot`, `config`, `db`, `domain`, `services`, and `integrations`.
- [x] Store runtime state, database files, and logs under `.cache`.
- [x] Add SQLx migrations for users, balances, markets, positions, trades, external snapshots, guild accounts, economy events, offers, and loans.
- [x] Add extensive structured logging around startup, commands, database work, pricing, and settlement.
- [x] Add startup and domain tests for config defaults, cache directory creation, pricing monotonicity, claims, and migrations.
- [x] Add Discord slash commands for bootstrap, native market flow, and Manifold tracking flow.
- [x] Move guild balances and claim cooldowns into `guild_accounts` and migrate legacy single-guild users where possible.
- [x] Route user-facing money rendering through the configurable currency formatter.
- [x] Add direct `/donate_money` and `/donate_shares` flows with atomic writes and guild-only boundaries.
- [x] Add guild-scoped `/offer_loan_money`, `/offer_loan_shares`, `/incoming_loans`, `/accept_loan`, `/decline_loan`, `/loan_status`, and `/repay_loan`.
- [x] Add background maintenance for share-offer expiry, pending-loan expiry, and overdue-loan defaulting.
- [x] Sync slash commands immediately per guild on restart and announce command catalog/version changes from a `.cache` startup manifest.
- [x] Add configurable bot auto-claim and selective bot loan acceptance behavior, market-depth histograms, and a first-pass pre-funded bond issuance system.
- [ ] Generalize existing peer offers onto `asset_offers` and expose `/offer_money`, `/incoming_offers`, `/accept_offer`, and `/decline_offer`.
- [ ] Add end-to-end tests for donations, loans, and guild-isolation behavior across multiple guilds.
