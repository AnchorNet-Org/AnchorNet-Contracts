//! Storage keys and typed accessors for the AnchorNet contract.
//!
//! Every live entry — both persistent and instance — has its TTL refreshed by
//! the typed accessors in this module, so business logic in `lib.rs` never has
//! to remember to bump TTL itself.
//!
//! # Storage Buckets
//!
//! The contract uses two distinct Soroban storage buckets, each with its own
//! TTL policy:
//!
//! - **Instance storage** (`env.storage().instance()`): Holds small, contract‑wide singleton configuration that is tightly coupled to the contract's code entry. The instance is a single archive unit: if it expires, *all* instance keys and the contract's Wasm entry expire together, bricking the contract until an explicit restore. Because that is the most severe failure mode, **every** instance accessor (read, write, and remove) refreshes the instance TTL via [`bump_instance`] using the shared threshold/bump constants. There is intentionally no per-key granularity at this layer.
//!   - `Admin`
//!   - `PendingAdmin`
//!   - `Operator`
//!   - `Paused`
//!   - `FeeBps`
//!   - `SettlementCount`
//!   - `SettlementExpiryLedgers`
//!
//! - **Persistent storage** (`env.storage().persistent()`): Stores per‑key data that can be archived and restored independently. Each entry is automatically extended on read/write via [`extend`] using the shared TTL bump policy.
//!   - `Anchor`, `Pool`, `Balance`, `Settlement`, `FeesAccrued`, `WaivedFeeVolume`, `AnchorList`, `AssetList`, `FeeWaiver`, `MinLiquidity`, `MaxSettlementAmount`, `AssetFee`
//!
//! # TTL Extension
//!
//! The single source of truth for both policies is the pair of constants
//! [`LIFETIME_THRESHOLD`] / [`BUMP_AMOUNT`]. Persistent entries are bumped
//! per-key through [`extend`]; the instance is bumped through
//! [`bump_instance`] (which is also what the public `extend_instance_ttl`
//! entrypoint calls). An entry is only bumped when it is actually present or
//! is being written — `extend_ttl` on a key that was never written would trap,
//! so getters that return a default for an absent key guard the bump behind an
//! existence check.
//!
//! This coverage model makes it impossible for new code to silently forget a
//! TTL bump: there is no raw `env.storage().persistent()/instance()` call in
//! business logic, and every accessor in this module owns its own bump. A
//! getter that returns a default for an absent key must still extend when the
//! key *is* present; tests in `test.rs` lock that in for every accessor.

use soroban_sdk::{contracttype, Address, Env, Symbol, Vec};

use crate::error::Error;
use crate::types::{AnchorStatus, Pool, Settlement};

const DAY_IN_LEDGERS: u32 = 17_280;
/// How long an entry's TTL is extended to on access (~30 days).
const BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
/// Extend the TTL once it drops below this threshold (~29 days).
const LIFETIME_THRESHOLD: u32 = BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Keys for every entry the contract stores.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The contract administrator.
    Admin,
    /// Whether an address is a registered anchor.
    Anchor(Address),
    /// The aggregate [`Pool`] for an asset.
    Pool(Symbol),
    /// A provider's liquidity balance in a given asset.
    Balance(Address, Symbol),
    /// Whether the contract is paused.
    Paused,
    /// The protocol fee in basis points.
    FeeBps,
    /// Monotonic counter for settlement ids.
    SettlementCount,
    /// A settlement record by id.
    Settlement(u64),
    /// Protocol fees accrued (and not yet collected) for an asset.
    FeesAccrued(Symbol),
    /// Forgone protocol fee revenue due to waivers.
    WaivedFeeVolume(Symbol),
    /// Ordered list of every address ever registered as an anchor.
    AnchorList,
    /// The address proposed to become the next administrator, if any.
    PendingAdmin,
    /// Whether an anchor is exempt from protocol settlement fees.
    FeeWaiver(Address),
    /// Number of ledgers after which a pending settlement may be reclaimed
    /// via `cancel_expired_settlement`. Zero disables expiry.
    SettlementExpiryLedgers,
    /// Ordered list of every asset that has ever had liquidity provided.
    AssetList,
    /// Minimum liquidity floor for an asset's pool; withdrawals that would
    /// leave the pool below this amount are rejected. Zero disables the
    /// check.
    MinLiquidity(Symbol),
    /// The contract operator, an address the admin may appoint to pause and
    /// unpause the contract without holding full admin rights.
    Operator,
    /// Maximum amount a single settlement may reserve for an asset. Zero
    /// disables the check.
    MaxSettlementAmount(Symbol),
    /// Per-asset protocol fee override, in basis points. Falls back to the
    /// global fee when unset.
    AssetFee(Symbol),
}

fn extend(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, LIFETIME_THRESHOLD, BUMP_AMOUNT);
}

/// Refreshes the TTL of the contract instance (and its Wasm code entry) using
/// the shared threshold/bump policy.
///
/// The instance is a single archive unit: all of `Admin`, `Paused`, `FeeBps`,
/// `SettlementCount`, etc. live inside it, so every instance accessor calls
/// this. Bumping is a no-op at the host level while the TTL is still above
/// [`LIFETIME_THRESHOLD`], so calling it on hot paths costs nothing when the
/// instance is fresh and prevents archival on cold paths.
fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(LIFETIME_THRESHOLD, BUMP_AMOUNT);
}

/// Extends the TTL of the contract instance and code, using the same
/// threshold/bump policy as individual persistent entries, so the contract
/// itself does not expire during a long period of inactivity.
///
/// This is the manual, permissioned entrypoint (admin/operator); every
/// instance accessor also calls [`bump_instance`] automatically, so this is
/// primarily useful to proactively refresh an instance that has seen no
/// traffic at all.
pub fn extend_instance_ttl(env: &Env) {
    bump_instance(env);
}

/// Returns `true` once an administrator has been set.
pub fn has_admin(env: &Env) -> bool {
    bump_instance(env);
    env.storage().instance().has(&DataKey::Admin)
}

/// Reads the administrator address.
///
/// Returns [`Error::NotInitialized`] when no administrator has been stored
/// yet, so callers can surface a typed, decodable error instead of trapping
/// on an unguarded `unwrap`. Every public entrypoint that needs the admin
/// either propagates this error with `?` (see `require_admin`) or treats it
/// as the contract's uninitialized state.
pub fn get_admin(env: &Env) -> Result<Address, Error> {
    bump_instance(env);
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)
}

/// Persists the administrator address in instance storage.
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
    bump_instance(env);
}

/// Reads the proposed next administrator.
///
/// Returns [`Error::NoPendingAdmin`] when no transfer is pending, so callers
/// can surface a typed, decodable error instead of trapping on an unguarded
/// `unwrap`. Every public entrypoint that needs the pending admin propagates
/// this error with `?`.
pub fn get_pending_admin(env: &Env) -> Result<Address, Error> {
    bump_instance(env);
    env.storage()
        .instance()
        .get(&DataKey::PendingAdmin)
        .ok_or(Error::NoPendingAdmin)
}

/// Returns `true` if an admin transfer has been proposed and not yet
/// accepted or overwritten.
// Kept as a typed storage probe for future entrypoints and integration users;
// the current contract flow reads the value directly and propagates its error.
#[allow(dead_code)]
pub fn has_pending_admin(env: &Env) -> bool {
    bump_instance(env);
    env.storage().instance().has(&DataKey::PendingAdmin)
}

/// Persists the proposed next administrator.
pub fn set_pending_admin(env: &Env, candidate: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::PendingAdmin, candidate);
    bump_instance(env);
}

/// Clears any proposed admin transfer.
pub fn clear_pending_admin(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingAdmin);
    bump_instance(env);
}

/// Returns `true` once an operator has been appointed.
pub fn has_operator(env: &Env) -> bool {
    bump_instance(env);
    env.storage().instance().has(&DataKey::Operator)
}

/// Reads the operator address.
///
/// Returns [`Error::NoOperator`] when no operator has been appointed, so
/// callers can surface a typed, decodable error instead of trapping on an
/// unguarded `unwrap`. Every public entrypoint that needs the operator
/// either propagates this error with `?` or guards with [`has_operator`]
/// first.
pub fn get_operator(env: &Env) -> Result<Address, Error> {
    bump_instance(env);
    env.storage()
        .instance()
        .get(&DataKey::Operator)
        .ok_or(Error::NoOperator)
}

/// Persists the operator address in instance storage.
pub fn set_operator(env: &Env, operator: &Address) {
    env.storage().instance().set(&DataKey::Operator, operator);
    bump_instance(env);
}

/// Removes the operator address from instance storage.
pub fn clear_operator(env: &Env) {
    env.storage().instance().remove(&DataKey::Operator);
    bump_instance(env);
}

/// Returns `true` if the contract is currently paused.
pub fn is_paused(env: &Env) -> bool {
    bump_instance(env);
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

/// Sets the paused flag.
pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&DataKey::Paused, &paused);
    bump_instance(env);
}

/// Reads the protocol fee in basis points (defaults to zero if unset).
pub fn get_fee_bps(env: &Env) -> u32 {
    bump_instance(env);
    env.storage().instance().get(&DataKey::FeeBps).unwrap_or(0)
}

/// Persists the protocol fee in basis points.
pub fn set_fee_bps(env: &Env, bps: u32) {
    env.storage().instance().set(&DataKey::FeeBps, &bps);
    bump_instance(env);
}

/// Returns `true` if `anchor` has been registered.
pub fn is_anchor(env: &Env, anchor: &Address) -> bool {
    let key = DataKey::Anchor(anchor.clone());
    if env.storage().persistent().has(&key) {
        extend(env, &key);
    }
    env.storage().persistent().get(&key).unwrap_or(false)
}

pub fn anchor_status(env: &Env, anchor: &Address) -> AnchorStatus {
    let key = DataKey::Anchor(anchor.clone());
    match env.storage().persistent().get::<DataKey, bool>(&key) {
        Some(true) => {
            extend(env, &key);
            AnchorStatus::Active
        }
        Some(false) => {
            extend(env, &key);
            AnchorStatus::Deregistered
        }
        None => AnchorStatus::NeverRegistered,
    }
}

/// Marks `anchor` as registered.
pub fn set_anchor(env: &Env, anchor: &Address) {
    set_anchor_flag(env, anchor, true);
}

/// Sets the registration flag for `anchor`.
pub fn set_anchor_flag(env: &Env, anchor: &Address, registered: bool) {
    let key = DataKey::Anchor(anchor.clone());
    env.storage().persistent().set(&key, &registered);
    extend(env, &key);
}

/// Reads the ordered list of every address ever registered as an anchor.
///
/// The list is append-only: deregistering an anchor does not remove it, so
/// callers must pair this with [`is_anchor`] to find currently active
/// anchors.
pub fn get_anchor_list(env: &Env) -> Vec<Address> {
    let key = DataKey::AnchorList;
    match env
        .storage()
        .persistent()
        .get::<DataKey, Vec<Address>>(&key)
    {
        Some(list) => {
            extend(env, &key);
            list
        }
        None => Vec::new(env),
    }
}

/// Appends `anchor` to the anchor list if it is not already present.
pub fn remember_anchor(env: &Env, anchor: &Address) {
    let mut list = get_anchor_list(env);
    if list.contains(anchor) {
        return;
    }
    list.push_back(anchor.clone());
    let key = DataKey::AnchorList;
    env.storage().persistent().set(&key, &list);
    extend(env, &key);
}

/// Reads the ordered list of every asset that has ever had liquidity
/// provided, in first-use order.
pub fn get_asset_list(env: &Env) -> Vec<Symbol> {
    let key = DataKey::AssetList;
    match env.storage().persistent().get::<DataKey, Vec<Symbol>>(&key) {
        Some(list) => {
            extend(env, &key);
            list
        }
        None => Vec::new(env),
    }
}

/// Appends `asset` to the asset list if it is not already present.
pub fn remember_asset(env: &Env, asset: &Symbol) -> bool {
    let mut list = get_asset_list(env);
    if list.contains(asset) {
        false
    } else {
        list.push_back(asset.clone());
        let key = DataKey::AssetList;
        env.storage().persistent().set(&key, &list);
        extend(env, &key);
        true
    }
}

/// Reads the [`Pool`] for `asset`, returning an empty pool if none exists.
pub fn get_pool(env: &Env, asset: &Symbol) -> Pool {
    let key = DataKey::Pool(asset.clone());
    match env.storage().persistent().get::<DataKey, Pool>(&key) {
        Some(pool) => {
            extend(env, &key);
            pool
        }
        None => Pool::empty(asset.clone()),
    }
}

/// Returns `true` if a pool entry exists for `asset`.
///
/// Extends the entry's TTL when present so the `pool_exists` / `pool` read
/// path, which never goes through a setter, cannot let an active pool archive
/// between liquidity events. Mirrors the existence-guarded bump used by
/// [`is_fee_waived`] and the other "set once, read often" getters.
pub fn has_pool(env: &Env, asset: &Symbol) -> bool {
    let key = DataKey::Pool(asset.clone());
    if env.storage().persistent().has(&key) {
        extend(env, &key);
    }
    env.storage().persistent().has(&key)
}

/// Persists `pool` for `asset`.
pub fn set_pool(env: &Env, asset: &Symbol, pool: &Pool) {
    let key = DataKey::Pool(asset.clone());
    env.storage().persistent().set(&key, pool);
    extend(env, &key);
}

/// Reads a provider's balance in `asset` (zero if none).
///
/// Extends the entry's TTL on a successful read. Balances are written on
/// provide/withdraw but read far more often — `balance`, `withdraw_*`,
/// `provider_share_bps`, `anchor_balances`, and the provide/withdraw internals
/// all go through here — so a balance that sits un-mutated while a settlement
/// is pending (an ordinary lifecycle) must not archive out from under the
/// position it backs. The `.has` guard leaves never-funded providers returning
/// `0` without attempting to extend an absent key.
pub fn get_balance(env: &Env, provider: &Address, asset: &Symbol) -> i128 {
    let key = DataKey::Balance(provider.clone(), asset.clone());
    if env.storage().persistent().has(&key) {
        extend(env, &key);
    }
    env.storage().persistent().get(&key).unwrap_or(0)
}

/// Persists a provider's balance in `asset`.
pub fn set_balance(env: &Env, provider: &Address, asset: &Symbol, amount: i128) {
    let key = DataKey::Balance(provider.clone(), asset.clone());
    env.storage().persistent().set(&key, &amount);
    extend(env, &key);
}

/// Reads the settlement id counter (zero before the first settlement).
pub fn get_settlement_count(env: &Env) -> u64 {
    bump_instance(env);
    env.storage()
        .instance()
        .get(&DataKey::SettlementCount)
        .unwrap_or(0)
}

/// Persists the settlement id counter.
pub fn set_settlement_count(env: &Env, count: u64) {
    env.storage()
        .instance()
        .set(&DataKey::SettlementCount, &count);
    bump_instance(env);
}

/// Reads a settlement by id, if it exists.
pub fn get_settlement(env: &Env, id: u64) -> Option<Settlement> {
    let key = DataKey::Settlement(id);
    let found = env.storage().persistent().get(&key);
    if found.is_some() {
        extend(env, &key);
    }
    found
}

/// Persists a settlement record.
pub fn set_settlement(env: &Env, settlement: &Settlement) {
    let key = DataKey::Settlement(settlement.id);
    env.storage().persistent().set(&key, settlement);
    extend(env, &key);
}

/// Returns `true` if `anchor` is exempt from protocol settlement fees.
///
/// Extends the entry's TTL on a successful read so that a waiver set once at
/// onboarding and only read afterward (via `quote_fee` / `open_settlement`, the
/// hot path) cannot silently archive between the rare admin rewrites
/// (issue #121). The `.has` guard avoids calling `extend_ttl` on an entry that
/// was never written, since the SDK requires the key to exist; unconfigured
/// anchors keep returning `false` untouched.
pub fn is_fee_waived(env: &Env, anchor: &Address) -> bool {
    let key = DataKey::FeeWaiver(anchor.clone());
    if env.storage().persistent().has(&key) {
        extend(env, &key);
    }
    env.storage().persistent().get(&key).unwrap_or(false)
}

/// Sets whether `anchor` is exempt from protocol settlement fees.
pub fn set_fee_waiver(env: &Env, anchor: &Address, waived: bool) {
    let key = DataKey::FeeWaiver(anchor.clone());
    env.storage().persistent().set(&key, &waived);
    extend(env, &key);
}

/// Returns `true` if the settlement expiry window has been explicitly
/// configured, including an explicit zero value that disables expiry.
pub fn has_settlement_expiry_ledgers(env: &Env) -> bool {
    bump_instance(env);
    env.storage()
        .instance()
        .has(&DataKey::SettlementExpiryLedgers)
}

/// Reads the settlement expiry window in ledgers (zero if never configured,
/// meaning settlements never expire).
pub fn get_settlement_expiry_ledgers(env: &Env) -> u32 {
    bump_instance(env);
    env.storage()
        .instance()
        .get(&DataKey::SettlementExpiryLedgers)
        .unwrap_or(0)
}

/// Persists the settlement expiry window in ledgers.
pub fn set_settlement_expiry_ledgers(env: &Env, ledgers: u32) {
    env.storage()
        .instance()
        .set(&DataKey::SettlementExpiryLedgers, &ledgers);
    bump_instance(env);
}

/// Reads the minimum liquidity floor configured for `asset` (zero, meaning
/// disabled, if never configured).
///
/// Extends the entry's TTL on a successful read so that heavily-read,
/// rarely-updated risk configuration cannot silently archive between writes
/// (issue #122). The `.has` guard avoids calling `extend_ttl` on an entry that
/// was never written, since the SDK requires the key to exist; unconfigured
/// assets keep returning `0` untouched.
pub fn get_min_liquidity(env: &Env, asset: &Symbol) -> i128 {
    let key = DataKey::MinLiquidity(asset.clone());
    if env.storage().persistent().has(&key) {
        extend(env, &key);
    }
    env.storage().persistent().get(&key).unwrap_or(0)
}

/// Returns `true` if a minimum liquidity floor has ever been configured for
/// `asset`, including an explicit zero floor that intentionally disables the
/// withdrawal check.
///
/// When present, the entry's TTL is extended just like [`has_pool`] and
/// [`has_max_settlement_amount`], so any future caller that relies on this
/// existence probe keeps the risk parameter alive. It is not currently wired to
/// an entrypoint (the value getter [`get_min_liquidity`] serves the existing
/// callers), but is kept as part of the storage accessor surface and is covered
/// by a TTL test.
#[allow(dead_code)]
pub fn has_min_liquidity(env: &Env, asset: &Symbol) -> bool {
    let key = DataKey::MinLiquidity(asset.clone());
    if env.storage().persistent().has(&key) {
        extend(env, &key);
    }
    env.storage().persistent().has(&key)
}

/// Persists the minimum liquidity floor for `asset`.
pub fn set_min_liquidity(env: &Env, asset: &Symbol, floor: i128) {
    let key = DataKey::MinLiquidity(asset.clone());
    env.storage().persistent().set(&key, &floor);
    extend(env, &key);
}

/// Reads the maximum settlement amount configured for `asset` (zero, meaning
/// disabled, if never configured).
///
/// Extends the entry's TTL on a successful read (see [`get_min_liquidity`] for
/// rationale — issue #122). The `.has` guard leaves unconfigured assets
/// returning `0` without touching storage.
pub fn get_max_settlement_amount(env: &Env, asset: &Symbol) -> i128 {
    let key = DataKey::MaxSettlementAmount(asset.clone());
    if env.storage().persistent().has(&key) {
        extend(env, &key);
    }
    env.storage().persistent().get(&key).unwrap_or(0)
}

/// Returns `true` if the maximum settlement amount for `asset` has ever been
/// explicitly configured, including when it was configured to zero to disable
/// the cap.
///
/// This distinguishes the default "never configured" zero returned by
/// [`get_max_settlement_amount`] from an administrator's explicit
/// `set_max_settlement_amount(asset, 0)` action. When present, the entry's TTL
/// is extended just like the value getter so read-only audit checks keep the
/// risk-parameter record alive.
pub fn has_max_settlement_amount(env: &Env, asset: &Symbol) -> bool {
    let key = DataKey::MaxSettlementAmount(asset.clone());
    let configured = env.storage().persistent().has(&key);
    if configured {
        extend(env, &key);
    }
    configured
}

/// Persists the maximum settlement amount for `asset`.
pub fn set_max_settlement_amount(env: &Env, asset: &Symbol, amount: i128) {
    let key = DataKey::MaxSettlementAmount(asset.clone());
    env.storage().persistent().set(&key, &amount);
    extend(env, &key);
}

/// Removes the minimum liquidity floor for `asset`, reverting to unset state.
pub fn clear_min_liquidity(env: &Env, asset: &Symbol) {
    let key = DataKey::MinLiquidity(asset.clone());
    env.storage().persistent().remove(&key);
}

/// Removes the maximum settlement amount for `asset`, reverting to unset state.
pub fn clear_max_settlement_amount(env: &Env, asset: &Symbol) {
    let key = DataKey::MaxSettlementAmount(asset.clone());
    env.storage().persistent().remove(&key);
}

/// Reads the per-asset fee override for `asset`, if one has been configured.
///
/// Extends the entry's TTL on a successful read (issue #122): the fee override
/// is looked up on every fee resolution while admins reconfigure it rarely, so
/// a long read-only period should not let it archive. Returns `None` untouched
/// when the override is absent — there is no entry to extend in that case.
pub fn get_asset_fee(env: &Env, asset: &Symbol) -> Option<u32> {
    let key = DataKey::AssetFee(asset.clone());
    let value = env.storage().persistent().get(&key);
    if value.is_some() {
        extend(env, &key);
    }
    value
}

/// Persists a per-asset fee override for `asset`.
pub fn set_asset_fee(env: &Env, asset: &Symbol, bps: u32) {
    let key = DataKey::AssetFee(asset.clone());
    env.storage().persistent().set(&key, &bps);
    extend(env, &key);
}

/// Removes any per-asset fee override for `asset`, reverting it to the
/// global fee.
pub fn clear_asset_fee(env: &Env, asset: &Symbol) {
    let key = DataKey::AssetFee(asset.clone());
    env.storage().persistent().remove(&key);
}

/// Reads the accrued (uncollected) protocol fees for `asset`.
///
/// Extends the entry's TTL on a successful read (issue #121): accrual is read
/// per settlement and inside `total_fees_accrued`'s loop, while writes only
/// happen on collection, so a heavily-read entry could otherwise archive and
/// understate collectible revenue. `total_fees_accrued` benefits automatically
/// once this getter is fixed. The `.has` guard mirrors [`is_fee_waived`] —
/// extending an unwritten entry would panic; unconfigured assets keep
/// returning `0` untouched.
pub fn get_fees_accrued(env: &Env, asset: &Symbol) -> i128 {
    let key = DataKey::FeesAccrued(asset.clone());
    if env.storage().persistent().has(&key) {
        extend(env, &key);
    }
    env.storage().persistent().get(&key).unwrap_or(0)
}

/// Persists the accrued protocol fees for `asset`.
pub fn set_fees_accrued(env: &Env, asset: &Symbol, amount: i128) {
    let key = DataKey::FeesAccrued(asset.clone());
    env.storage().persistent().set(&key, &amount);
    extend(env, &key);
}

/// Reads the forgone protocol fee volume for `asset`.
///
/// Extends the entry's TTL on a successful read. The volume is written only
/// when a waived anchor opens a settlement, but is read on the reporting path
/// (`waived_fee_volume`, `total_waived_fee_volume`), so a long gap between
/// waived settlements could otherwise let the entry archive and zero out the
/// reported forgone revenue. The `.has` guard leaves assets with no waiver
/// activity returning `0` without touching an absent key.
pub fn get_waived_fee_volume(env: &Env, asset: &Symbol) -> i128 {
    let key = DataKey::WaivedFeeVolume(asset.clone());
    if env.storage().persistent().has(&key) {
        extend(env, &key);
    }
    env.storage().persistent().get(&key).unwrap_or(0)
}

/// Persists the forgone protocol fee volume for `asset`.
pub fn set_waived_fee_volume(env: &Env, asset: &Symbol, amount: i128) {
    let key = DataKey::WaivedFeeVolume(asset.clone());
    env.storage().persistent().set(&key, &amount);
    extend(env, &key);
}
