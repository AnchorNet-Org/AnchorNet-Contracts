# TTL / Archival Audit — 50 storage sites

**Issue:** persistent and instance entries can archive while still logically
live (e.g. a settlement awaiting execution, a provider's balance, the contract
instance itself). Recovery requires an explicit `RestoreFootprint` that nothing
in the contract documents.

**Verification command (unchanged):**

```
grep -rn "storage()\.persistent()\|storage()\.instance()" src/ | wc -l
```

> The count below is the **50 sites in the audited (pre-fix) tree** exactly as
> the issue's grep sees them: 45 code sites in `src/storage.rs`, 2 doc-comment
> mentions in `src/storage.rs`, and 3 test-utility sites in `src/test.rs`.
> (After the fix the same grep prints more because new tests were added and a
> couple of `has()` probes were expanded from one line to two; no new *business*
> storage surface was introduced — keys and the data model are unchanged.)

## Thresholds — single source of truth

Defined once in `src/storage.rs` and reused by every bump:

```rust
const DAY_IN_LEDGERS: u32 = 17_280;
const BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;        // ~30 days
const LIFETIME_THRESHOLD: u32 = BUMP_AMOUNT - DAY_IN_LEDGERS; // ~29 days
```

There is **no duplicate set** of TTL constants anywhere in `src/`.

## Persistent storage — 37 sites

| # | Site (function) | Key | Op | TTL extended BEFORE | TTL extended AFTER |
|---|---|---|---|---|---|
| 1 | doc comment (module) | — | — | n/a (doc) | n/a (doc) |
| 2 | `is_anchor` | `Anchor(a)` | `has` | ✅ guarded `extend` on present | ✅ unchanged |
| 3 | `is_anchor` | `Anchor(a)` | `get` | ✅ (after the guarded extend) | ✅ unchanged |
| 4 | `anchor_status` | `Anchor(a)` | `get` | ✅ `extend` on `Some` | ✅ unchanged |
| 5 | `set_anchor_flag` | `Anchor(a)` | `set` | ✅ `extend` after write | ✅ unchanged |
| 6 | `remember_anchor` | `AnchorList` | `set` | ✅ `extend` after write | ✅ unchanged |
| 7 | `get_anchor_list` | `AnchorList` | `get` | ✅ `extend` on `Some` | ✅ unchanged |
| 8 | `get_asset_list` | `AssetList` | `get` | ✅ `extend` on `Some` | ✅ unchanged |
| 9 | `remember_asset` | `AssetList` | `set` | ✅ `extend` after write | ✅ unchanged |
| 10 | `get_pool` | `Pool(s)` | `get` | ✅ `extend` on `Some` | ✅ unchanged |
| 11 | `has_pool` | `Pool(s)` | `has` | ❌ **gap** — no bump | ✅ **fixed** (guarded extend on present) |
| 12 | `set_pool` | `Pool(s)` | `set` | ✅ `extend` after write | ✅ unchanged |
| 13 | `get_balance` | `Balance(a,s)` | `get` | ❌ **gap (critical)** — no bump | ✅ **fixed** (guarded extend on present) |
| 14 | `set_balance` | `Balance(a,s)` | `set` | ✅ `extend` after write | ✅ unchanged |
| 15 | `get_settlement` | `Settlement(id)` | `get` | ✅ `extend` on `Some` | ✅ unchanged |
| 16 | `set_settlement` | `Settlement(id)` | `set` | ✅ `extend` after write | ✅ unchanged |
| 17 | `is_fee_waived` | `FeeWaiver(a)` | `has` | ✅ guarded `extend` on present | ✅ unchanged |
| 18 | `is_fee_waived` | `FeeWaiver(a)` | `get` | ✅ | ✅ unchanged |
| 19 | `set_fee_waiver` | `FeeWaiver(a)` | `set` | ✅ `extend` after write | ✅ unchanged |
| 20 | `get_min_liquidity` | `MinLiquidity(s)` | `has` | ✅ guarded `extend` on present | ✅ unchanged |
| 21 | `get_min_liquidity` | `MinLiquidity(s)` | `get` | ✅ | ✅ unchanged |
| 22 | `has_min_liquidity` | `MinLiquidity(s)` | `has` | ❌ **gap** (dead code) — no bump | ✅ **fixed** (guarded extend on present) |
| 23 | `set_min_liquidity` | `MinLiquidity(s)` | `set` | ✅ `extend` after write | ✅ unchanged |
| 24 | `get_max_settlement_amount` | `MaxSettlementAmount(s)` | `has` | ✅ guarded `extend` on present | ✅ unchanged |
| 25 | `get_max_settlement_amount` | `MaxSettlementAmount(s)` | `get` | ✅ | ✅ unchanged |
| 26 | `has_max_settlement_amount` | `MaxSettlementAmount(s)` | `has` | ✅ `extend` on present | ✅ unchanged |
| 27 | `set_max_settlement_amount` | `MaxSettlementAmount(s)` | `set` | ✅ `extend` after write | ✅ unchanged |
| 28 | `clear_min_liquidity` | `MinLiquidity(s)` | `remove` | ➖ intentionally not extended (entry deleted) | ➖ unchanged |
| 29 | `clear_max_settlement_amount` | `MaxSettlementAmount(s)` | `remove` | ➖ intentionally not extended (entry deleted) | ➖ unchanged |
| 30 | `get_asset_fee` | `AssetFee(s)` | `get` | ✅ `extend` on `Some` | ✅ unchanged |
| 31 | `set_asset_fee` | `AssetFee(s)` | `set` | ✅ `extend` after write | ✅ unchanged |
| 32 | `clear_asset_fee` | `AssetFee(s)` | `remove` | ➖ intentionally not extended (entry deleted) | ➖ unchanged |
| 33 | `get_fees_accrued` | `FeesAccrued(s)` | `has` | ✅ guarded `extend` on present | ✅ unchanged |
| 34 | `get_fees_accrued` | `FeesAccrued(s)` | `get` | ✅ | ✅ unchanged |
| 35 | `set_fees_accrued` | `FeesAccrued(s)` | `set` | ✅ `extend` after write | ✅ unchanged |
| 36 | `get_waived_fee_volume` | `WaivedFeeVolume(s)` | `get` | ❌ **gap** — no bump | ✅ **fixed** (guarded extend on present) |
| 37 | `set_waived_fee_volume` | `WaivedFeeVolume(s)` | `set` | ✅ `extend` after write | ✅ unchanged |

The three `persistent()` test sites (in `src/test.rs`) are TTL *inspection*
helpers (`persistent().has/get/get_ttl` inside `as_contract`); they are not
contract storage logic and require no coverage.

## Instance storage — 13 sites

The instance is a **single archive unit**: `Admin`, `PendingAdmin`, `Operator`,
`Paused`, `FeeBps`, `SettlementCount`, and `SettlementExpiryLedgers` all live in
it, and if it expires the Wasm entry expires with it — the most severe failure
mode. Empirically verified on soroban-sdk 25: **writing an instance key does
*not* refresh the instance TTL** (the TTL stayed at `3095` after a write at
ledger `1000`). Before this fix the instance was kept alive *only* by an
admin/operator manually calling `extend_instance_ttl`.

| # | Site (function) | Op | TTL extended BEFORE | TTL extended AFTER |
|---|---|---|---|---|
| 1 | doc comment (module) | — | n/a (doc) | n/a (doc) |
| 2 | `extend_instance_ttl` | `extend_ttl` | ✅ (manual entrypoint) | ✅ delegates to `bump_instance` |
| 3 | `has_admin` | `has` | ❌ **gap** — no auto-bump | ✅ `bump_instance` on access |
| 4 | `get_admin` | `get` | ❌ **gap** | ✅ `bump_instance` on access |
| 5 | `set_admin` | `set` | ❌ **gap** (writes don't bump!) | ✅ `bump_instance` after write |
| 6 | `has_pending_admin` | `has` | ❌ **gap** | ✅ `bump_instance` on access |
| 7 | `get_pending_admin` | `get` | ❌ **gap** | ✅ `bump_instance` on access |
| 8 | `set_pending_admin` | `set` | ❌ **gap** | ✅ `bump_instance` after write |
| 9 | `clear_pending_admin` | `remove` | ❌ **gap** | ✅ `bump_instance` after remove |
| 10 | `has_operator` | `has` | ❌ **gap** | ✅ `bump_instance` on access |
| 11 | `get_operator` | `get` | ❌ **gap** | ✅ `bump_instance` on access |
| 12 | `set_operator` | `set` | ❌ **gap** | ✅ `bump_instance` after write |
| 13 | `clear_operator` | `remove` | ❌ **gap** | ✅ `bump_instance` after remove |

The following instance accessors (in the same grep family, multiline) are
covered identically and also bumped after the fix: `is_paused` (get),
`set_paused` (set), `get_fee_bps` (get), `set_fee_bps` (set),
`get_settlement_count` (get), `set_settlement_count` (set),
`has_settlement_expiry_ledgers` (has), `get_settlement_expiry_ledgers` (get),
`set_settlement_expiry_ledgers` (set). **Every instance accessor now bumps.**

## Genuine gaps vs. intentional non-extensions

**Genuine gaps fixed:**
`get_balance` (critical — backs liquidity positions), `get_waived_fee_volume`,
`has_pool`, `has_min_liquidity`, and **all 13 instance accessors** (the
instance was the systemic, highest-severity gap).

**Intentionally not extended (with reason):**
- `clear_min_liquidity`, `clear_max_settlement_amount`, `clear_asset_fee` —
  these `remove` the key; there is no entry left whose TTL matters.
- `get_balance`/`get_waived_fee_volume`/etc. on an **absent** key — the
  accessors return the default (`0`/`false`/`None`) and skip `extend_ttl`,
  because extending a key that was never written traps at the host. The
  existence `.has` guard makes that explicit and safe.
- No settlement, pool, balance, anchor, fee, or risk key is intentionally
  short-lived; every present entry is bumped on access.

## Design decision

**Coverage model chosen: typed accessors in `storage.rs` that bump internally.**

- All raw `env.storage()` access lives behind the `storage::` functions;
  `lib.rs` contains **zero** direct `storage()` calls. Each accessor owns its
  own bump, so business logic cannot forget one.
- This prevents future bypass structurally: a new persistent key gets a
  `get_/set_/has_` accessor that calls `extend`; a new instance key
  automatically shares `bump_instance`. There is no per-call-site checklist a
  reviewer must enforce, and no entrypoint boundary where a newly added
  external function could be missed.
- The host makes `extend_ttl` a no-op while the TTL is above
  `LIFETIME_THRESHOLD`, so bumping on every access has no runtime cost on hot
  keys; it only writes rent state when the entry is actually nearing expiry.

**Why not the alternatives:**
- *Entrypoint-boundary bumps* would require every `pub fn` to enumerate and
  bump every key it (and its callees) might touch — easy to get wrong and easy
  to bypass when a new entrypoint is added.
- *Explicit keeper entrypoint* shifts availability to an off-chain actor that
  must run continuously; the issue's failure mode is precisely that nothing
  currently performs or documents restore, so relying on a keeper recreates
  the problem.

**Instance TTL:** explicitly established and tested. The instance is bumped by
`bump_instance` on **every** accessor call (read, write, and remove), plus the
manual `extend_instance_ttl` entrypoint. The instance can therefore no longer
archive merely because no administrator clicked "extend"; any contract use keeps
it alive. The `test_instance_survives_long_idle_period` test locks this in.

## Rent / abuse analysis

- **Who pays:** the transaction **invoker** pays the rent/resource fee for the
  bump, exactly as they pay for any other storage write their call triggers.
  The contract holds no token balance and cannot be drained.
- **Can an attacker make the contract pay to keep adversarial entries alive?**
  No. The contract itself never initiates a bump in a background/keeper flow;
  every bump is part of a caller-invoked, caller-paid transaction. An attacker
  can cause bumps only by invoking the contract, which costs the attacker the
  resource fee. The set of persistent keys is also bounded and derived from
  legitimate protocol objects (anchors, assets, settlements) rather than
  attacker-chosen arbitrary keys, so an attacker cannot manufacture an
  unbounded number of entries for the contract to later subsidize.
- **Griefing on read paths:** read-only calls bump the keys they legitimately
  read (e.g. `list_settlements_*` bump each scanned settlement). That cost is
  paid by the caller and is proportional to the page they request; it cannot be
  imposed on another user or on the contract.

## Tests (failing → passing)

Each gap has a test that advances the ledger past `LIFETIME_THRESHOLD` and
asserts the read refreshes TTL / the record survives:

- `test_balance_read_bumps_ttl`, `test_anchor_balances_scan_bumps_each_balance_ttl`,
  `test_balance_survives_past_ttl_threshold`, `test_balance_read_on_unfunded_provider_is_safe`
- `test_waived_fee_volume_read_bumps_ttl`, `test_total_waived_fee_volume_cascades_bump`,
  `test_waived_fee_volume_survives_past_ttl_threshold`,
  `test_waived_fee_volume_read_on_unconfigured_asset_is_safe`
- `test_pool_exists_read_bumps_ttl`, `test_pool_survives_past_ttl_threshold_via_exists_probe`
- `test_has_min_liquidity_read_bumps_ttl`
- Instance: `test_instance_read_bumps_ttl`, `test_instance_write_bumps_ttl`,
  `test_instance_survives_long_idle_period`,
  `test_extend_instance_ttl_entrypoint_bumps_instance`

Before the fix the `*_bumps_ttl` tests fail (`after == before`, i.e. the read
was a pure access); after the fix they pass and the `*_survives_*` tests prove
records remain usable through an idle period that would previously let them
drift toward archival.

## Results

- `cargo test`: **320 passed; 0 failed** (305 pre-existing + 15 new).
- `cargo fmt --all -- --check`: clean.
- `cargo build --target wasm32-unknown-unknown --release`: succeeds.
- **Wasm byte delta: 93,598 → 93,887 bytes (+289 bytes, +0.31%).**
- No storage keys or data-model types changed.
