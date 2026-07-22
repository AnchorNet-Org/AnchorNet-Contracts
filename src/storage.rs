//! Storage keys and typed accessors for the AnchorNet contract.
//!
//! All persistent entries use the `persistent` storage with a TTL that is
//! extended on every read/write so that active pools are not archived.

use soroban_sdk::{contracttype, Address, Env, Symbol, Vec};

use crate::types::{Pool, Settlement};

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

/// Extends the TTL of the contract instance and code, using the same
/// threshold/bump policy as individual persistent entries, so the contract
/// itself does not expire during a long period of inactivity.
pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(LIFETIME_THRESHOLD, BUMP_AMOUNT);
}

/// Returns `true` once an administrator has been set.
pub fn has_admin(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
}

/// Reads the administrator address. Panics if uninitialized — callers should
/// guard with [`has_admin`] first.
pub fn get_admin(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Admin).unwrap()
}

/// Persists the administrator address in instance storage.
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

/// Returns `true` if an admin transfer has been proposed and not yet
/// accepted or overwritten.
pub fn has_pending_admin(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::PendingAdmin)
}

/// Reads the proposed next administrator. Panics if none is pending —
/// callers should guard with [`has_pending_admin`] first.
pub fn get_pending_admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::PendingAdmin)
        .unwrap()
}

/// Persists the proposed next administrator.
pub fn set_pending_admin(env: &Env, candidate: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::PendingAdmin, candidate);
}

/// Clears any proposed admin transfer.
pub fn clear_pending_admin(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingAdmin);
}

/// Returns `true` once an operator has been appointed.
pub fn has_operator(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Operator)
}

/// Reads the operator address. Panics if none is appointed — callers should
/// guard with [`has_operator`] first.
pub fn get_operator(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Operator).unwrap()
}

/// Persists the operator address in instance storage.
pub fn set_operator(env: &Env, operator: &Address) {
    env.storage().instance().set(&DataKey::Operator, operator);
}

/// Returns `true` if the contract is currently paused.
pub fn is_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

/// Sets the paused flag.
pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&DataKey::Paused, &paused);
}

/// Reads the protocol fee in basis points (defaults to zero if unset).
pub fn get_fee_bps(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::FeeBps).unwrap_or(0)
}

/// Persists the protocol fee in basis points.
pub fn set_fee_bps(env: &Env, bps: u32) {
    env.storage().instance().set(&DataKey::FeeBps, &bps);
}

/// Returns `true` if `anchor` has been registered.
pub fn is_anchor(env: &Env, anchor: &Address) -> bool {
    let key = DataKey::Anchor(anchor.clone());
    env.storage().persistent().get(&key).unwrap_or(false)
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
pub fn remember_asset(env: &Env, asset: &Symbol) {
    let mut list = get_asset_list(env);
    if list.contains(asset) {
        return;
    }
    list.push_back(asset.clone());
    let key = DataKey::AssetList;
    env.storage().persistent().set(&key, &list);
    extend(env, &key);
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
pub fn has_pool(env: &Env, asset: &Symbol) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Pool(asset.clone()))
}

/// Persists `pool` for `asset`.
pub fn set_pool(env: &Env, asset: &Symbol, pool: &Pool) {
    let key = DataKey::Pool(asset.clone());
    env.storage().persistent().set(&key, pool);
    extend(env, &key);
}

/// Reads a provider's balance in `asset` (zero if none).
pub fn get_balance(env: &Env, provider: &Address, asset: &Symbol) -> i128 {
    let key = DataKey::Balance(provider.clone(), asset.clone());
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

/// Reads the settlement expiry window in ledgers (zero if never configured,
/// meaning settlements never expire).
pub fn get_settlement_expiry_ledgers(env: &Env) -> u32 {
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

/// Persists the maximum settlement amount for `asset`.
pub fn set_max_settlement_amount(env: &Env, asset: &Symbol, amount: i128) {
    let key = DataKey::MaxSettlementAmount(asset.clone());
    env.storage().persistent().set(&key, &amount);
    extend(env, &key);
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
pub fn get_waived_fee_volume(env: &Env, asset: &Symbol) -> i128 {
    let key = DataKey::WaivedFeeVolume(asset.clone());
    env.storage().persistent().get(&key).unwrap_or(0)
}

/// Persists the forgone protocol fee volume for `asset`.
pub fn set_waived_fee_volume(env: &Env, asset: &Symbol, amount: i128) {
    let key = DataKey::WaivedFeeVolume(asset.clone());
    env.storage().persistent().set(&key, &amount);
    extend(env, &key);
}
