# anchornet-contracts

Soroban smart contracts for **AnchorNet** — the liquidity coordination network for Stellar anchors. This repo contains on-chain logic for liquidity pools, routing metadata, and settlement hooks.

## Overview

- **Stack:** Rust, [Soroban SDK](https://soroban.stellar.org/docs)
- **Network:** Stellar (Soroban)

## Prerequisites

- [Rust](https://rustup.rs/) (stable, with `rustfmt`)
- Optional: [Soroban CLI](https://soroban.stellar.org/docs/getting-started/setup#install-the-soroban-cli) for deployment and local testing

## Setup

```bash
# Clone the repo (or use your fork)
git clone <repo-url>
cd anchornet-contracts

# Install Rust if needed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Check formatting, build, and test
cargo fmt --all -- --check
cargo build
cargo test
```

## Project structure

- `src/lib.rs` – contract entrypoint and public interface
- `src/error.rs` – error codes returned to clients
- [`docs/ADMIN.md`](docs/ADMIN.md) – privileged admin/operator roles, lifecycle, and security properties
- [`docs/ERRORS.md`](docs/ERRORS.md) – stable error-code reference and originating entrypoints
- [`docs/PAGINATION.md`](docs/PAGINATION.md) – stable pagination semantics reference and worked examples
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
| `list_anchors(start, limit)` | – | Page through currently registered anchors |
| `anchor_count()` | – | Read the number of currently registered anchors |
| `provide_liquidity(provider, asset, amount)` | provider | Add liquidity to a pool |
| `provide_liquidity_multi(provider, requests)` | provider | Add liquidity to several assets in one call and authorization; validates the whole batch (no duplicate assets) before applying any of it |
| `withdraw_liquidity(provider, asset, amount)` | provider | Remove liquidity from a pool |
| `withdraw_all_liquidity(provider, asset)` | provider | Withdraw a provider's entire balance in one call |
| `withdraw_liquidity_multi(provider, requests)` | provider | Withdraw from several assets in one call and authorization; validates the whole batch (no duplicate assets) before applying any of it |
| `deregister_anchor(anchor)` | admin | Remove an anchor from the approved set |
| `pool(asset)` | – | Read aggregate pool state |
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
| `is_paused()` | – | Read the paused flag |
| `set_operator(operator)` | admin | Appoint an operator that may pause/unpause but cannot change fees or admin |
| `operator()` | – | Read the currently appointed operator |
| `is_operator(address)` | – | Check whether an address is the currently appointed operator |
| `extend_instance_ttl(caller)` | admin or operator | Extend the contract instance/code TTL so it survives long inactivity |
| `set_fee(bps)` | admin | Set the protocol fee in basis points (max 1000) |
| `fee()` / `quote_fee(asset, amount)` | – | Read the global fee rate / preview the effective fee for an asset |
| `max_fee_bps()` | – | Read the maximum fee `set_fee`/`set_asset_fee` will accept |
| `set_asset_fee(asset, bps)` | admin | Override the protocol fee for one asset, independent of the global rate |
| `clear_asset_fee(asset)` | admin | Remove an asset's fee override, reverting it to the global rate |
| `asset_fee(asset)` | – | Read the effective fee for an asset (its override, or the global rate) |
| `collect_fees(asset)` | admin | Collect accrued protocol fees for an asset |
| `fees_accrued(asset)` | – | Read uncollected fees for an asset |
| `total_fees_accrued()` | – | Read the sum of uncollected fees across every asset ever funded |
| `set_fee_waiver(anchor, waived)` | admin | Grant or revoke a fee waiver for a registered anchor |
| `is_fee_waived(anchor)` | – | Check whether an anchor is exempt from settlement fees |
| `list_fee_waived_anchors(start, limit)` | – | Page through currently registered anchors with an active fee waiver |
| `fee_waived_anchor_count()` | – | Read the number of currently registered anchors with an active fee waiver |
| `version()` | – | Read the contract interface version |

Fee calculations intentionally use floor division:
`floor(amount * bps / 10_000)`. As a result, tiny settlements can have a
zero fee even when the configured rate is nonzero. For example, at 1 bps,
amounts below 10,000 quote and accrue a fee of 0, while an amount of 10,000
produces a fee of 1. This rounding behavior is an accepted protocol tradeoff.

### Settlement

| Function | Auth | Description |
|----------|------|-------------|
| `open_settlement(anchor, asset, amount)` | anchor | Reserve pool liquidity, returns a settlement id |
| `execute_settlement(id)` | admin | Finalize a settlement and accrue its fee |
| `cancel_settlement(id)` | anchor | Cancel and return reserved liquidity to the pool |
| `cancel_expired_settlement(id)` | – | Reclaim a timed-out pending settlement's liquidity to the pool |
| `set_settlement_expiry_ledgers(ledgers)` | admin | Set the ledger window after which a pending settlement may be reclaimed (0 disables) |
| `settlement_expiry_ledgers()` | – | Read the settlement expiry window in ledgers |
| `is_settlement_expired(id)` | – | Check whether a pending settlement has passed the expiry window, without reclaiming it |
| `settlement(id)` | – | Read a settlement record |
| `settlement_count()` | – | Read the number of settlements |
| `list_settlements(start, limit)` | – | Page through settlements |
| `list_settlements_by_anchor(anchor, start, limit)` | – | Page through settlements opened by one anchor |
| `list_settlements_by_asset(asset, start, limit)` | – | Page through settlements in one asset |
| `list_settlements_by_status(status, start, limit)` | – | Page through settlements in a given lifecycle state |
| `settlement_count_by_status(status)` | – | Count every settlement in a given lifecycle state (no pagination) |
| `total_settled_amount(status)` | – | Sum settled `amount` across every settlement in a given lifecycle state |
| `contract_info()` | – | One-call snapshot of version, paused flag, fee, and anchor/asset/settlement counts |

`cancel_expired_settlement` requires no authorization: it only ever returns
liquidity to the shared pool it was reserved from, never to an external
party, so anyone (including an off-chain keeper) may call it once a pending
settlement has passed the configured expiry window.

`pause` and `unpause` take an explicit `caller` argument (Soroban contracts
have no implicit sender) that must be either the admin or the appointed
operator; the operator role is scoped to this one lifecycle switch and
carries no ability to change the fee, the admin, or any other admin-only
setting.

### Events

- `("init",)` – contract initialized
- `("admin",)` – administrator changed (via `set_admin` or `accept_admin`)
- `("propose",)` – admin transfer proposed
- `("anchor", anchor)` / `("deanchor", anchor)` – anchor registered / removed
- `("provide", provider, asset)` – liquidity provided
- `("withdraw", provider, asset)` – liquidity withdrawn
- `("paused",)` – paused flag changed; data is `true` when pausing, `false` when unpausing
- `("fee",)` – protocol fee changed
- `("waiver", anchor)` – anchor fee waiver granted or revoked
- `("settle", anchor, asset)` – settlement opened
- `("executed", id)` / `("cancelled", id)` – settlement finalized / cancelled
- `("expired", id)` – settlement reclaimed after timing out
- `("expiry",)` – settlement expiry window changed
- `("collect", asset)` – fees collected
- `("minliq", asset)` – asset minimum liquidity floor changed
- `("operator",)` – operator appointed or replaced

## Contract metadata

The compiled wasm embeds `Name` and `Description` entries (via
`contractmeta!`) so tooling that inspects the deployed contract can identify
it without an off-chain registry.

## Commands

| Command | Description |
|--------|-------------|
| `cargo build` | Build the contract |
| `cargo test` | Run unit tests |
| `cargo fmt --all` | Format code |
| `cargo fmt --all -- --check` | Check formatting (CI) |

## Contributing

1. Fork the repo and create a branch from `main`.
2. Make changes; keep formatting with `cargo fmt --all`.
3. Run `cargo fmt --all -- --check`, `cargo build`, and `cargo test`.
4. Open a pull request. CI will run format check, build, and tests.

## License

MIT
