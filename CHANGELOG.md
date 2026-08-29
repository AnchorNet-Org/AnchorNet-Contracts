# Changelog

All notable changes to the AnchorNet contracts are documented here.

## [Unreleased]

> Contributors: add new user-facing or codebase changes under the relevant
> category below using the [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
> style and concise bullet points.

### Added

### Changed

- **Build tooling:** the Makefile is now the single source of truth for build,
  test, and format operations — CI invokes `make fmt-check`, `make build`, and
  `make test` instead of calling cargo directly, and the README documents the
  same make targets. Added `rust-toolchain.toml` pinning the stable toolchain
  so local and CI builds use the same Rust version.

### Deprecated

### Removed

### Fixed

- **Errors:** the storage accessors `get_admin`, `get_pending_admin`, and
  `get_operator` now return typed contract errors (`NotInitialized`,
  `NoPendingAdmin`, `NoOperator`) when the corresponding entry is missing,
  instead of trapping with an undecodable host panic. Public entrypoint
  behavior is unchanged — the same errors, now enforced at the accessor.
- **Errors:** the four list-pagination `unwrap()`s in `list_anchors`,
  `list_fee_waived_anchors`, `list_assets`, and `anchor_balances` were
  audited and annotated as guarded invariants (in-bounds by the loop bound).
- **CI:** added a guard that rejects new unguarded `unwrap()`/`expect()`/
  `panic!` sites in contract source.

### Security

## [0.9.0]

### Added

- **Liquidity:** `provide_liquidity_multi`, batch provisioning across
  several assets in one call and authorization, sharing a new internal
  `do_provide` helper with the refactored `provide_liquidity`.
- **Liquidity:** `anchor_balances(provider, start, limit)` to page through a
  provider's non-zero balances across every known asset, instead of calling
  `balance` once per asset off-chain.
- **Settlements:** `total_settled_amount(status)`, summing settled amounts
  across the full history for a given lifecycle state, alongside
  `settlement_count_by_status`.

## [0.8.0]

### Added

- **Settlements:** `settlement_count_by_status`, a full-history aggregate
  count complementing the paginated `list_settlements_by_status`.
- **Liquidity:** `withdraw_liquidity_multi`, batch withdrawal across several
  assets in one call and authorization; validates the whole batch (no
  duplicate assets, sufficient balance, minimum liquidity floor) before
  applying any of it, via a new `Error::DuplicateAssetInBatch`.
- **Operations:** `contract_info`, a one-call snapshot (version, paused,
  fee, anchor/asset/settlement counts) for off-chain dashboards; `max_fee_bps`
  to read the fee cap without hardcoding it off-chain.

## [0.7.0]

### Added

- **Settlements:** `set_max_settlement_amount`/`max_settlement_amount` — an
  admin-configurable per-asset cap on the amount a single `open_settlement`
  call may reserve (disabled by default), enforced with a new
  `Error::AboveMaxSettlementAmount`.
- **Fees:** `set_asset_fee`/`clear_asset_fee`/`asset_fee` — an admin-configurable
  per-asset fee override, independent of the global rate, respected by both
  `quote_fee` (now asset-scoped) and `open_settlement`'s fee calculation. A
  fee waiver still takes precedence over any override.
- **Operations:** `extend_instance_ttl`, callable by the admin or operator, to
  extend the contract instance/code TTL and avoid archival during long
  inactivity.

### Changed

- `quote_fee` now takes an `asset` parameter so its preview matches the fee
  actually charged for that asset (a breaking signature change, acceptable
  pre-1.0).

## [0.6.0]

### Added

- **Operator role:** `set_operator` lets the admin appoint an operator that
  may call `pause` / `unpause` on the admin's behalf, without gaining the
  ability to change the fee, the admin, or any other admin-only setting.
  `operator` reads the currently appointed operator; `is_operator` checks a
  specific address. `pause` and `unpause` now take an explicit `caller`
  argument — accepted if it is either the admin or the operator — since a
  Soroban contract has no implicit sender to fall back on. A caller that is
  neither now returns the (previously unused) `Error::NotAuthorized` instead
  of a generic auth failure. Emits `("operator",)` on appointment.
- **Minimum liquidity floor:** `set_min_liquidity` lets the admin configure,
  per asset, a floor below which `withdraw_liquidity` and
  `withdraw_all_liquidity` refuse to drain a pool (zero, the default,
  disables the check). `min_liquidity` reads the configured floor. Emits
  `("minliq", asset)` on change.
- **Settlement status queries:** `list_settlements_by_status` pages through
  settlements filtered by lifecycle state (`Pending` / `Executed` /
  `Cancelled` / `Expired`), mirroring `list_settlements_by_anchor` and
  `list_settlements_by_asset`. `is_settlement_expired` reports whether a
  pending settlement has passed the configured expiry window without
  mutating any state, letting off-chain keepers check before attempting a
  `cancel_expired_settlement` call that might otherwise fail.
- **Aggregate totals:** `total_liquidity_all` and `total_fees_accrued` sum
  `total_liquidity` and `fees_accrued` respectively across every asset ever
  funded (per `list_assets`), sparing callers from summing pages
  themselves. `asset_count` reads the number of distinct assets ever
  funded directly, mirroring `anchor_count`.

## [0.5.0]

### Added

- **Settlement expiry:** `set_settlement_expiry_ledgers` lets the admin
  configure a ledger window after which a still-pending settlement can be
  reclaimed (zero, the default, disables expiry). `settlement_expiry_ledgers`
  reads the current window. `cancel_expired_settlement` reclaims a timed-out
  settlement's reserved liquidity back to the pool once the window has
  elapsed; it requires no authorization since it can only ever return value
  to the shared pool, allowing off-chain keepers to sweep stale settlements.
  Settlements now record the ledger sequence they were opened at
  (`opened_at`) and gain a new `Expired` status distinct from manual
  cancellation. Emits `("expiry",)` on configuration change and
  `("expired", id)` on reclaim.
- **Fee waiver enumeration:** `list_fee_waived_anchors` pages through
  registered anchors that currently hold an active fee waiver, mirroring
  `list_anchors`.
- **Asset enumeration:** `list_assets` pages through every asset that has
  ever had liquidity provided, in first-use order, letting callers discover
  which assets to query via `pool` or `collect_fees` without an off-chain
  indexer.
- **Full-balance exit:** `withdraw_all_liquidity` withdraws a provider's
  entire balance in one call, sharing its pool/balance bookkeeping with
  `withdraw_liquidity`.
- **Batch anchor onboarding:** `register_anchors` registers a batch of
  anchors atomically — if any address is already registered or repeated
  within the batch, the whole call fails and no anchor is registered.

## [0.4.0]

### Added

- **Per-asset settlement queries:** `list_settlements_by_asset` pages through
  the settlements opened in a single asset, mirroring the existing
  `list_settlements_by_anchor`.
- **Anchor pagination:** `list_anchors` now takes `start` / `limit` and pages
  through the registration history, skipping deregistered anchors without
  counting them toward `limit`, mirroring how `list_settlements` already
  paginates. `anchor_count` is unaffected and still reports the full active
  count.
- **Fee waivers:** `set_fee_waiver` lets the admin exempt a specific
  registered anchor from protocol settlement fees; `is_fee_waived` reads the
  flag. Waived anchors are charged zero fee in `open_settlement` regardless of
  the configured rate. Emits a `("waiver", anchor)` event on change.
- **Contract metadata:** the compiled wasm now embeds `Name` and
  `Description` entries via `contractmeta!`.
- **Tests:** boundary coverage proving `provide_liquidity`, `quote_fee`, and
  `open_settlement` panic on i128 overflow rather than silently wrapping,
  relying on the crate's `overflow-checks = true` profile setting.

## [0.3.0]

### Added

- **Anchor enumeration:** `list_anchors` returns every currently registered
  anchor in registration order; `anchor_count` reads how many are active.
  Deregistered anchors are excluded but re-registration does not duplicate an
  anchor in the list.
- **Two-step admin transfer:** `propose_admin` / `accept_admin` offer a safer
  alternative to the existing single-step `set_admin`, requiring the proposed
  address to explicitly accept before control changes. `pending_admin` reads
  the outstanding proposal, if any.
- **Settlement queries:** `list_settlements_by_anchor` pages through the
  settlements opened by a single anchor.
- **Events:** `set_admin` (and the new `accept_admin`) now emit an `("admin",)`
  event on administrator change, closing a gap where admin transfers were
  previously silent; `propose_admin` emits `("propose",)`.

## [0.2.0]

### Added

- **Admin & lifecycle:** `pause` / `unpause` with an `is_paused` view; all
  liquidity and settlement mutations are blocked while paused.
- **Protocol fees:** configurable fee in basis points (`set_fee`, capped at
  10%), `fee` / `quote_fee` views, per-asset accrual, and admin `collect_fees`.
- **Settlement engine:** `open_settlement` reserves pool liquidity and returns
  an id; `execute_settlement` finalizes and accrues the fee; `cancel_settlement`
  returns reserved liquidity. Query via `settlement`, `settlement_exists`,
  `settlement_count`, and paginated `list_settlements`.
- **Anchor lifecycle:** `deregister_anchor` to remove an approved anchor.
- **Introspection:** `version` and `is_initialized` getters.
- Events for pause, fee changes, settlement lifecycle, and fee collection.

## [0.1.0]

### Added

- Initial liquidity pool registry: contract initialization, admin management,
  anchor registration, `provide_liquidity` / `withdraw_liquidity`, and pool /
  balance queries.
