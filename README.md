anchornet-contracts
Soroban smart contracts for AnchorNet — the liquidity coordination network for Stellar anchors. This repo contains on-chain logic for liquidity pools, routing metadata, and settlement hooks.

Overview
Stack: Rust, Soroban SDK
Network: Stellar (Soroban)
Prerequisites
Rust (stable, with rustfmt) — pinned via rust-toolchain.toml
make (GNU Make) — the Makefile is the single source of truth for build operations; CI runs the same targets
Optional: Soroban CLI for deployment and local testing
Optional: wasm32-unknown-unknown target for make wasm (rustup target add wasm32-unknown-unknown)
Setup
Bash

Clone the repo (or use your fork)
git clone <repo-url>
cd anchornet-contracts

Install Rust if needed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

Check formatting, build, and test — the same targets CI runs
make fmt-check
make build
make test

text


## Project structure

- `src/lib.rs` – contract entrypoint and public interface
- `src/error.rs` – error codes returned to clients
- [`docs/ADMIN.md`](docs/ADMIN.md) – privileged admin/operator roles, lifecycle, and security properties
- [`docs/ERRORS.md`](docs/ERRORS.md) – stable error-code reference and originating entrypoints
- [`docs/PAGINATION.md`](docs/PAGINATION.md) – stable pagination semantics reference and worked examples
- [`docs/EVENTS.md`](docs/EVENTS.md) – event topics, argument types, and indexer integration guide
- `src/types.rs` – on-chain data types (`Pool`)
- `src/storage.rs` – storage keys and TTL-aware accessors
- `src/events.rs` – event publishing helpers
- `src/test.rs` – unit tests
- `Cargo.toml` – dependencies and crate config

## Contract interface

The `AnchornetContract` tracks per-asset liquidity pools funded by registered
anchors. The off-chain indexer subscribes to the emitted events to mirror pool
state.

| Function | Auth | Description |
|----------|------|-------------|
| `initialize(admin)` | once | Set the contract administrator |
| `admin()` | – | Read the current administrator |
| `set_admin(new_admin)` | admin | Transfer administration in a single step |
| `propose_admin(candidate)` | admin | Propose `candidate` as the next administrator |
| `accept_admin(candidate)` | candidate | Accept a pending admin transfer |
| `pending_admin()` | – | Read the proposed next administrator, if any |
| `register_anchor(anchor)` | admin | Approve an anchor as a liquidity provider |
| `register_anchors(anchors)` | admin | Approve a batch of anchors atomically in one call |
| `is_anchor(anchor)` | – | Check whether an address is registered |
| `anchor_status(anchor)` | – | Read the registration status (`NeverRegistered`, `Active`, or `Deregistered`) of an address |
| `list_anchors(start, limit)` | – | Page through currently registered anchors |
| `anchor_count()` | – | Read the number of currently registered anchors |
| `provide_liquidity(provider, asset, amount)` | provider | Add liquidity to a pool |
| `provide_liquidity_multi(provider, requests)` | provider | Add liquidity to several assets in one call and authorization; validates the whole batch (no duplicate assets) before applying any of i[...]
| `withdraw_liquidity(provider, asset, amount)` | provider | Remove liquidity from a pool |
| `withdraw_all_liquidity(provider, asset)` | provider | Withdraw a provider's entire balance in one call |
| `withdraw_liquidity_multi(provider, requests)` | provider | Withdraw from several assets in one call and authorization; validates the whole batch (no duplicate assets) before applying any of it [...]
| `deregister_anchor(anchor)` | admin | Remove an anchor from the approved set |
| `pool(asset)` | – | Read aggregate pool state |
| `pool_exists(asset)` | – | Check whether a pool entry exists for an asset (i.e. liquidity has ever been provided for it) |
| `total_liquidity(asset)` | – | Read total liquidity for an asset |
| `total_liquidity_all()` | – | Read the sum of total liquidity across every asset ever funded |
| `balance(provider, asset)` | – | Read a provider's balance |
| `anchor_balances(provider, start, limit)` | – | Page through a provider's non-zero balances across every known asset |
| `list_assets(start, limit)` | – | Page through every asset that has ever had liquidity provided |
| `asset_count()` | – | Read the number of distinct assets that have ever had liquidity provided |
| `set_min_liquidity(asset, floor)` | admin | Set the minimum liquidity floor an asset's pool may not be withdrawn below (0 disables) |
| `min_liquidity(asset)` | – | Read the minimum liquidity floor configured for an asset |
| `set_max_settlement_amount(asset, amount)` | admin | Cap the amount a single settlement may reserve for an asset (0 disables) |
| `max_settlement_amount(asset)` | – | Read the maximum settlement amount configured for an asset |

### Admin & lifecycle

| Function | Auth | Description |
|----------|------|-------------|
| `pause(caller)` / `unpause(caller)` | admin or operator | Halt or resume liquidity & settlement mutations |
| `set_operator(operator)` | admin | Appoint an operator that may pause/unpause but cannot change fees or admin |
| `clear_operator()` | admin | Revoke the operator role entirely |
| `renounce_operator(caller)` | operator (self) | Operator voluntarily steps down without admin involvement |
| `operator()` | – | Read the currently appointed operator |
| `is_operator(address)` | – | Check whether an address is the currently appointed operator |
| `extend_instance_ttl(caller)` | admin or operator | Extend the contract instance/code TTL so it survives long inactivity |

> **Note:** `extend_instance_ttl` only refreshes the **instance** storage bucket. Persistent entries (e.g., `Anchor`, `Pool`, `Balance`, etc.) have independent TTLs managed by per‑key `extend` c[...]

| `set_fee(bps)` | admin | Set the protocol fee in basis points (max 1000) |
| `fee()` / `quote_fee(asset, amount)` | – | Read the global fee rate / preview the effective fee for an asset |
| `max_fee_bps()` | – | Read the maximum fee `set_fee`/`set_asset_fee` will accept |
| `set_asset_fee(asset, bps)` | admin | Override the protocol fee for one asset, independent of the global rate |
| `clear_asset_fee(asset)` | admin | Remove an asset's fee override, reverting it to the global rate |
| `asset_fee(asset)` | – | Read the effective fee for an asset (its override, or the global rate) |
| `has_asset_fee_override(asset)` | – | Read whether an explicit fee override is configured for `asset`, distinguishing an admin-set `0` bps override from the absence of any override |
| `collect_fees(asset)` | admin | Collect accrued protocol fees for an asset |
| `fees_accrued(asset)` | – | Read uncollected fees for an asset |
| `total_fees_accrued()` | – | Read the sum of uncollected fees across every asset ever funded |
| `set_fee_waiver(anchor, waived)` | admin | Grant or revoke a fee waiver for a registered anchor |
| `is_fee_waived(anchor)` | – | Check whether an anchor is exempt from settlement fees |
| `list_fee_waived_anchors(start, limit)` | – | Page through currently registered anchors with an active fee waiver |
| `fee_waived_anchor_count()` | – | Read the number of currently registered anchors with an active fee waiver |
| `version()` | – | Read the contract interface version |

Fee calculations intentionally use floor division:
floor(amount * bps / 10_000). As a result, tiny settlements can have a
zero fee even when the configured rate is nonzero. For example, at 1 bps,
amounts below 10,000 quote and accrue a fee of 0, while an amount of 10,000
produces a fee of 1. This rounding behavior is an accepted protocol tradeoff.

### Fee override visibility
`asset_fee(asset)` collapses the per-asset fee override into the effective rate (`Option<u32>` → `u32`), so it cannot distinguish an explicit `0` bps override (`set_asset_fee(asset, 0)`) from the absence of any override when the global fee is also `0`. `has_asset_fee_override(asset)` resolves this ambiguity by exposing whether the override entry exists in storage (`Some(_)`), independent of its value.

Settlement
Function	Auth	Description
open_settlement(anchor, asset, amount)	anchor	Reserve pool liquidity, returns a settlement id
execute_settlement(id)	admin	Finalize a settlement and accrue its fee
cancel_settlement(id)	anchor	Cancel and return reserved liquidity to the pool
cancel_expired_settlement(id)	–	Reclaim a timed-out pending settlement's liquidity to the pool
set_settlement_expiry_ledgers(ledgers)	admin	Set the ledger window after which a pending settlement may be reclaimed (0 disables)
settlement_expiry_ledgers()	–	Read the settlement expiry window in ledgers
settlement_exists(id)	–	Check whether a settlement exists
settlement_status(id)	–	Read only the status of a settlement, or SettlementNotFound if missing
is_settlement_pending(id)	–	Check whether a settlement exists and its status is Pending
is_settlement_expired(id)	–	Check whether a pending settlement has passed the expiry window, without reclaiming it
settlement(id)	–	Read a settlement record
settlement_count()	–	Read the number of settlements
list_settlements(start, limit)	–	Page through settlements
list_settlements_by_anchor(anchor, start, limit)	–	Page through settlements opened by one anchor
list_settlements_by_asset(asset, start, limit)	–	Page through settlements in one asset
list_settlements_by_anchor_and_asset(anchor, asset, start, limit)	–	Page through settlements matching both anchor and asset
list_settlements_by_status(status, start, limit)	–	Page through settlements in a given lifecycle state
settlement_count_by_status(status)	–	Count every settlement in a given lifecycle state (no pagination)
total_settled_amount(status)	–	Sum settled amount across every settlement in a given lifecycle state
contract_info()	–	One-call snapshot of version, paused flag, fee, and anchor/asset/settlement counts
Settlement lifecycle (state machine)
SettlementStatus has four variants: Pending, Executed, Cancelled, Expired. Only the three one-way transitions shown below are valid. All three destination states are terminal — no further transition is possible from Executed, Cancelled, or Expired, and any attempt to transition from them will be rejected with InvalidSettlementState.

From	To	Function	Authorization	Condition
Pending	Executed	execute_settlement(id)	admin only	Settlement must exist and be Pending
Pending	Cancelled	cancel_settlement(id)	settlement's anchor (auth required)	Settlement must exist and be Pending
Pending	Expired	cancel_expired_settlement(id)	permissionless (any caller)	settlement_expiry_ledgers > 0 and ledger >= opened_at + expiry
mermaid

stateDiagram-v2
    [*] --> Pending : open_settlement(anchor, asset, amount) [anchor auth]
    Pending --> Executed : execute_settlement(id) [admin only]
    Pending --> Cancelled : cancel_settlement(id) [anchor auth]
    Pending --> Expired : cancel_expired_settlement(id) [permissionless, after expiry window]
    Executed --> [*] : terminal (no exit)
    Cancelled --> [*] : terminal (no exit)
    Expired --> [*] : terminal (no exit)
Terminal-state finality: Executed, Cancelled, and Expired are mutually exclusive and final. The contract enforces this by rejecting any transition call (execute_settlement, cancel_settlement, cancel_expired_settlement) on a settlement whose status is not exactly Pending (Error::InvalidSettlementState). The executable proof of this behavior is covered by the settlement lifecycle regression tests in src/test.rs (e.g. test_execute_cancelled_settlement_fails, test_execute_expired_settlement_fails, test_cancel_executed_fails, test_cancel_expired_settlement_rejects_already_executed, test_cancel_expired_settlement_rejects_before_expiry), which verify that already-terminal settlements cannot be re-transitioned.

cancel_expired_settlement requires no authorization: it only ever returns
liquidity to the shared pool it was reserved from, never to an external
party, so anyone (including an off-chain keeper) may call it once a pending
settlement has passed the configured expiry window.

pause and unpause take an explicit caller argument (Soroban contracts
have no implicit sender) that must be either the admin or the appointed
operator; the operator role is scoped to this one lifecycle switch and
carries no ability to change the fee, the admin, or any other admin-only
setting. Note that appointing the admin as its own operator is a supported
(if redundant) dual-role configuration.

### Storage & TTL

The contract uses Soroban's two storage buckets with independent TTL (time to
live) policies:

**Instance storage** (`env.storage().instance()`) holds small, contract-wide
singleton configuration tightly coupled to the contract code entry. These
entries are not subject to per-key TTL extensions and are expected to persist
as long as the contract itself does.

| Key | Type | Description |
|-----|------|-------------|
| `Admin` | `Address` | Contract administrator |
| `PendingAdmin` | `Address` | Proposed next administrator (optional) |
| `Operator` | `Address` | Appointed pause/unpause delegate (optional) |
| `Paused` | `bool` | Whether mutations are halted |
| `FeeBps` | `u32` | Global protocol fee rate |
| `SettlementCount` | `u64` | Monotonic settlement-id counter |
| `SettlementExpiryLedgers` | `u32` | Settlement expiry window in ledgers |

**Persistent storage** (`env.storage().persistent()`) stores per-key data
that can be archived and restored independently. Every persistent entry is
automatically extended on each read or write using the contract's TTL bump
policy.

| Key | Type | Description |
|-----|------|-------------|
| `Anchor(Address)` | `bool` | Anchor registration flag |
| `Pool(Symbol)` | `Pool` | Per-asset liquidity aggregate (total, providers) |
| `Balance(Address, Symbol)` | `i128` | Provider's liquidity balance per asset |
| `Settlement(u64)` | `Settlement` | Settlement record by id |
| `FeesAccrued(Symbol)` | `i128` | Uncollected protocol fees per asset |
| `WaivedFeeVolume(Symbol)` | `i128` | Forgone fee revenue due to waivers per asset |
| `AnchorList` | `Vec<Address>` | Append-only registration history |
| `AssetList` | `Vec<Symbol>` | Append-only first-use asset list |
| `FeeWaiver(Address)` | `bool` | Fee-waiver flag per anchor |
| `MinLiquidity(Symbol)` | `i128` | Withdrawal floor per asset |
| `MaxSettlementAmount(Symbol)` | `i128` | Per-settlement cap per asset |
| `AssetFee(Symbol)` | `u32` | Per-asset fee override (optional) |

**TTL parameters:**

- `DAY_IN_LEDGERS = 17,280` — one stellar ledger day (~5 seconds per ledger)
- `BUMP_AMOUNT = 30 * DAY_IN_LEDGERS` — entries are extended to ~30 days
  on every access (`set_pool`, `get_pool`, `set_settlement`, `get_settlement`,
  etc.)
- `LIFETIME_THRESHOLD = BUMP_AMOUNT - DAY_IN_LEDGERS` — extension fires once
  the remaining lifetime drops below ~29 days

This means a Pool or Settlement entry that is actively read or written
(e.g. by liquidity provision, settlement opening, or querying) has its TTL
refreshed to ~30 days from each access. Entries that are never touched
again — a Pool whose asset has been drained of all liquidity, or a terminal
Settlement that has been Executed, Cancelled, or Expired — will eventually
archive once their TTL elapses (~30 days after the last access), at which
point the entry disappears from on-chain storage and a subsequent read
returns the default (empty Pool / `None` settlement). The storage accessors
in `src/storage.rs` handle the fallback uniformly.

**`extend_instance_ttl` only refreshes the instance bucket.** It has no
effect on any persistent entry. Pool and Settlement entries rely on their
own per-key `extend` calls, which are triggered naturally by read/write
traffic. Read-only operational queries (`pool`, `settlement`,
`total_liquidity`, `max_settlement_amount`, `min_liquidity`, `is_fee_waived`,
`asset_fee`, etc.) all extend the TTL of the entry they read, so that
heavily-queried but rarely-updated risk configuration does not silently
archive between admin rewrites (see issue #122 and the `has`-guarded
extenders in `src/storage.rs`).

In practice this means:
- **`Pool(asset)`** — stays alive as long as liquidity is actively provided
  or withdrawn, or as long as anyone queries it. An abandoned pool entry
  expires ~30 days after its last access.
- **`Settlement(id)`** — stays alive while the settlement is being actively
  queried or transitioned (open, execute, cancel, expire). Finalized
  settlements that are never queried again eventually expire from storage,
  but the settlement id counter and aggregate views (`settlement_count`,
  `total_settled_amount`, etc.) in instance storage remain accurate.
- **`Balance(provider, asset)`** — stays alive while the provider remains
  active or their balance is queried.

Operator permission boundary
The table below lists every gated entrypoint and which guard function
it calls in src/lib.rs, so integrators and delegates can
verify the boundary without reading individual doc comments.

require_admin_or_operator — admin or operator may call

Entrypoint	Description
pause(caller)	Halt liquidity & settlement mutations
unpause(caller)	Resume after a pause
extend_instance_ttl(caller)	Extend contract instance/code TTL
require_admin — admin only (operator excluded)

Entrypoint	Description
set_admin(new_admin)	Transfer administration (single-step)
propose_admin(candidate)	Initiate a two-step admin transfer
set_operator(operator)	Appoint or replace the operator
renounce_operator(caller)	Self-service operator exit (caller must be the operator, not admin)

Note: `renounce_operator` is gated by a custom check (not `require_admin` or `require_admin_or_operator`): the caller must be the current operator and provide their own authorization. The admin cannot renounce on the operator's behalf.
register_anchor(anchor)	Approve a new liquidity provider
register_anchors(anchors)	Batch-approve liquidity providers
deregister_anchor(anchor)	Remove an anchor from the approved set
set_fee(bps)	Set the global protocol fee
set_asset_fee(asset, bps)	Override the fee for one asset
clear_asset_fee(asset)	Remove an asset's fee override
set_fee_waiver(anchor, waived)	Grant or revoke a fee waiver
collect_fees(asset)	Collect accrued protocol fees
set_min_liquidity(asset, floor)	Set the minimum liquidity floor
set_max_settlement_amount(asset, amount)	Cap per-settlement reserve size
set_settlement_expiry_ledgers(ledgers)	Set the settlement expiry window
execute_settlement(id)	Finalize a pending settlement
Note: The three-entry require_admin_or_operator list and the
fifteen-entry require_admin list are derived directly from the
corresponding call sites in src/lib.rs. When a new entrypoint is added,
check which guard it calls and update this table accordingly.

| Entrypoint | Description |
|---|---|
| `set_admin(new_admin)` | Transfer administration (single-step) |
| `propose_admin(candidate)` | Initiate a two-step admin transfer |
| `set_operator(operator)` | Appoint or replace the operator |
| `register_anchor(anchor)` | Approve a new liquidity provider |
| `register_anchors(anchors)` | Batch-approve liquidity providers |
| `deregister_anchor(anchor)` | Remove an anchor from the approved set |
| `set_fee(bps)` | Set the global protocol fee |
| `set_asset_fee(asset, bps)` | Override the fee for one asset |
| `clear_asset_fee(asset)` | Remove an asset's fee override |
| `set_fee_waiver(anchor, waived)` | Grant or revoke a fee waiver |
| `collect_fees(asset)` | Collect accrued protocol fees |
| `set_min_liquidity(asset, floor)` | Set the minimum liquidity floor |
| `set_max_settlement_amount(asset, amount)` | Cap per-settlement reserve size |
| `set_settlement_expiry_ledgers(ledgers)` | Set the settlement expiry window |
| `execute_settlement(id)` | Finalize a pending settlement |

> **Note:** The three-entry `require_admin_or_operator` list and the
> fifteen-entry `require_admin` list are derived directly from the
> corresponding call sites in `src/lib.rs`. When a new entrypoint is added,
> check which guard it calls and update this table accordingly.

### Events

For detailed event documentation, including argument shapes, emission sites, and indexer integration guidance, see [`docs/EVENTS.md`](docs/EVENTS.md).

**Event topics at a glance:**

- `("init",)` – contract initialized
- `("admin",)` – administrator changed (via `set_admin` or `accept_admin`)
- `("propose",)` – admin transfer proposed
- `("anchor", anchor)` / `("deanchor", anchor)` – anchor registered / removed
- `("provide", provider, asset)` – liquidity provided
- `("onboarded", asset)` – first liquidity provision for a new asset
- `("withdraw", provider, asset)` – liquidity withdrawn
- `("exited", provider, asset)` – provider's balance reached zero (full exit); fires after `("withdraw", …)` on the same transaction, only when the remaining balance is exactly 0
- `("paused",)` – paused flag flipped (data: `bool`)
- `("fee",)` – fee rate changed (data: `u32` bps)
- `("waiver", anchor)` – anchor fee waiver granted or revoked
- `("settle", anchor, asset)` – settlement opened
- `("executed", id)` / `("cancelled", id)` – settlement finalized / cancelled
- `("expired", id)` – settlement reclaimed after timing out
- `("expiry",)` – settlement expiry window changed
- `("collect", asset)` – fees collected
- `("minliq", asset)` – minimum liquidity floor configured
- `("maxamt", asset)` – maximum settlement amount configured
- `("assetfee", asset)` – asset-specific fee override set (data: `u32` bps)
- `("feeclear", asset)` – asset-specific fee override cleared
- `("operator",)` – operator appointed or replaced
- `("op_clear",)` – operator role revoked

## Contract metadata

The compiled wasm embeds `Name` and `Description` entries (via
`contractmeta!`) so tooling that inspects the deployed contract can identify
it without an off-chain registry.

## Commands

The Makefile is the single source of truth for build operations: CI
(`.github/workflows/ci.yml`) invokes the same targets you run locally, so a
command that works locally behaves identically in CI.

| Command | Description |
|--------|-------------|
| `make fmt-check` | Check formatting (`cargo fmt --all -- --check`) — CI runs this |
| `make build` | Build the contract for native testing — CI runs this |
| `make test` | Run unit tests — CI runs this |
| `make fmt` | Format code in place |
| `make wasm` | Build the optimized wasm artifact for deployment (`cargo build --target wasm32-unknown-unknown --release`); requires `rustup target add wasm32-unknown-unknown` |
| `make clean` | Remove build artifacts |

The wasm build is intentionally not part of CI yet; wiring it in is tracked
separately. Until then, `make wasm` is the local way to produce the
deployment artifact.

## Contributing

1. Fork the repo and create a branch from `main`.
2. Make changes; keep formatting with `make fmt`.
3. If modifying public contract functions, parameters, return types, data structures, error codes, or events, review and complete the [`Public API Compatibility Checklist`](docs/PUBLIC_API_CHECKLIST.md).
4. Run `make fmt-check`, `make build`, and `make test` — the same targets CI runs.
5. Open a pull request. CI will run the same `make fmt-check`, `make build`, and `make test` targets.

## License

MIT