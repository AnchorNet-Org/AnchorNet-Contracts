//! AnchorNet Soroban smart contracts.
//!
//! This crate contains on-chain logic for the AnchorNet liquidity coordination
//! network (liquidity pools, routing metadata, settlement hooks).

use soroban_sdk::{contract, contractimpl, contractmeta, Address, Env, Symbol, Vec};

mod error;
mod events;
mod storage;
mod types;

pub use error::Error;
pub use types::{ContractInfo, Pool, Settlement, SettlementStatus};

/// Maximum protocol fee that can be configured: 1000 bps (10%).
const MAX_FEE_BPS: u32 = 1_000;
/// Basis-points denominator.
const BPS_DENOMINATOR: i128 = 10_000;

contractmeta!(
    key = "Description",
    val = "AnchorNet liquidity coordination contract"
);
contractmeta!(key = "Name", val = "anchornet-contracts");

/// The AnchorNet liquidity coordination contract.
///
/// Tracks per-asset liquidity pools funded by registered anchors so the
/// off-chain routing layer can settle cross-anchor payments against a shared,
/// auditable on-chain balance.
#[contract]
pub struct AnchornetContract;

#[contractimpl]
impl AnchornetContract {
    /// Initializes the contract and sets the administrator.
    ///
    /// Can only be called once; subsequent calls return
    /// [`Error::AlreadyInitialized`].
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if storage::has_admin(&env) {
            return Err(Error::AlreadyInitialized);
        }
        storage::set_admin(&env, &admin);
        events::initialized(&env, &admin);
        Ok(())
    }

    /// Returns the contract interface version.
    pub fn version() -> u32 {
        9
    }

    /// Returns `true` if the contract has been initialized.
    pub fn is_initialized(env: Env) -> bool {
        storage::has_admin(&env)
    }

    /// Returns the current administrator address.
    pub fn admin(env: Env) -> Result<Address, Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Ok(storage::get_admin(&env))
    }

    /// Transfers administration to `new_admin`. Requires authorization from the
    /// current administrator.
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        Self::require_admin(&env)?;
        storage::set_admin(&env, &new_admin);
        events::admin_changed(&env, &new_admin, false);
        Ok(())
    }

    /// Proposes `candidate` as the next administrator. Admin only.
    ///
    /// The transfer only takes effect once `candidate` calls
    /// [`accept_admin`](Self::accept_admin), a safer two-step alternative to
    /// [`set_admin`](Self::set_admin) that guards against transferring
    /// control to an unreachable or mistyped address.
    ///
    /// Returns [`Error::InvalidAdminCandidate`] if `candidate` is the same as
    /// the current administrator, since a no-op proposal would produce events
    /// with no actual authority change.
    pub fn propose_admin(env: Env, candidate: Address) -> Result<(), Error> {
        Self::require_admin(&env)?;
        if candidate == storage::get_admin(&env) {
            return Err(Error::InvalidAdminCandidate);
        }
        storage::set_pending_admin(&env, &candidate);
        events::admin_proposed(&env, &candidate);
        Ok(())
    }

    /// Returns the address proposed to become the next administrator, if
    /// any.
    pub fn pending_admin(env: Env) -> Result<Address, Error> {
        if !storage::has_pending_admin(&env) {
            return Err(Error::NoPendingAdmin);
        }
        Ok(storage::get_pending_admin(&env))
    }

    /// Accepts a pending admin transfer proposed via
    /// [`propose_admin`](Self::propose_admin). Requires authorization from
    /// `candidate`, who must match the proposed address.
    pub fn accept_admin(env: Env, candidate: Address) -> Result<(), Error> {
        if !storage::has_pending_admin(&env) {
            return Err(Error::NoPendingAdmin);
        }
        if storage::get_pending_admin(&env) != candidate {
            return Err(Error::NotPendingAdmin);
        }
        candidate.require_auth();

        storage::set_admin(&env, &candidate);
        storage::clear_pending_admin(&env);
        events::admin_changed(&env, &candidate, true);
        Ok(())
    }

    /// Appoints `operator` as the contract operator, a role that may call
    /// [`pause`](Self::pause) and [`unpause`](Self::unpause) on the admin's
    /// behalf but cannot change the fee, the admin, or any other admin-only
    /// setting. Calling again replaces any previously appointed operator.
    /// Admin only.
    pub fn set_operator(env: Env, operator: Address) -> Result<(), Error> {
        Self::require_admin(&env)?;
        storage::set_operator(&env, &operator);
        events::operator_changed(&env, &operator);
        Ok(())
    }

    /// Revokes the operator role, returning the contract to an operator-less
    /// state where no address has delegated pause/unpause authority.
    /// Admin only.
    pub fn clear_operator(env: Env) -> Result<(), Error> {
        Self::require_admin(&env)?;
        storage::clear_operator(&env);
        events::operator_cleared(&env);
        Ok(())
    }

    /// Returns the currently appointed operator, or [`Error::NoOperator`] if
    /// none has been appointed.
    pub fn operator(env: Env) -> Result<Address, Error> {
        if !storage::has_operator(&env) {
            return Err(Error::NoOperator);
        }
        Ok(storage::get_operator(&env))
    }

    /// Returns `true` if `address` is the currently appointed operator.
    pub fn is_operator(env: Env, address: Address) -> bool {
        storage::has_operator(&env) && storage::get_operator(&env) == address
    }

    /// Pauses the contract, blocking liquidity and settlement mutations.
    /// Requires authorization from `caller`, who must be either the admin or
    /// the appointed [`operator`](Self::operator).
    pub fn pause(env: Env, caller: Address) -> Result<(), Error> {
        Self::require_admin_or_operator(&env, &caller)?;
        storage::set_paused(&env, true);
        events::paused_changed(&env, true);
        Ok(())
    }

    /// Resumes the contract after a pause. Requires authorization from
    /// `caller`, who must be either the admin or the appointed
    /// [`operator`](Self::operator).
    pub fn unpause(env: Env, caller: Address) -> Result<(), Error> {
        Self::require_admin_or_operator(&env, &caller)?;
        storage::set_paused(&env, false);
        events::paused_changed(&env, false);
        Ok(())
    }

    /// Returns `true` if the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        storage::is_paused(&env)
    }

    /// Extends the TTL of the contract instance and code so it does not
    /// expire during a long period of inactivity. Callable by the admin or
    /// the appointed [`operator`](Self::operator); has no effect on
    /// individual entries (those extend automatically on access).
    pub fn extend_instance_ttl(env: Env, caller: Address) -> Result<(), Error> {
        Self::require_admin_or_operator(&env, &caller)?;
        storage::extend_instance_ttl(&env);
        Ok(())
    }

    /// Sets the protocol fee in basis points (max 1000 = 10%). Admin only.
    pub fn set_fee(env: Env, bps: u32) -> Result<(), Error> {
        Self::require_admin(&env)?;
        if bps > MAX_FEE_BPS {
            return Err(Error::InvalidFee);
        }
        storage::set_fee_bps(&env, bps);
        events::fee_changed(&env, bps);
        Ok(())
    }

    /// Returns the protocol fee in basis points.
    pub fn fee(env: Env) -> u32 {
        storage::get_fee_bps(&env)
    }

    /// Returns the maximum protocol fee, in basis points, that
    /// [`set_fee`](Self::set_fee) and [`set_asset_fee`](Self::set_asset_fee)
    /// will accept, so off-chain clients don't need to hardcode it.
    pub fn max_fee_bps() -> u32 {
        MAX_FEE_BPS
    }

    /// Previews the protocol fee charged for settling `amount` of `asset` at
    /// the current fee rate (respecting any [`asset_fee`](Self::asset_fee)
    /// override), without changing any state. Returns [`Error::InvalidAmount`]
    /// when `amount` is zero or negative.
    pub fn quote_fee(env: Env, asset: Symbol, amount: i128) -> Result<i128, Error> {
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        Self::calculate_fee(
            amount,
            Self::effective_fee_bps(&env, &asset),
        )
    }

    /// Grants or revokes a protocol fee waiver for `anchor`. While waived,
    /// settlements opened by `anchor` are charged zero fee regardless of the
    /// configured rate. Admin only; `anchor` must be a registered anchor.
    pub fn set_fee_waiver(env: Env, anchor: Address, waived: bool) -> Result<(), Error> {
        Self::require_admin(&env)?;
        if !storage::is_anchor(&env, &anchor) {
            return Err(Error::AnchorNotRegistered);
        }
        storage::set_fee_waiver(&env, &anchor, waived);
        events::fee_waiver_changed(&env, &anchor, waived);
        Ok(())
    }

    /// Returns `true` if `anchor` is currently exempt from protocol
    /// settlement fees.
    pub fn is_fee_waived(env: Env, anchor: Address) -> bool {
        storage::is_fee_waived(&env, &anchor)
    }

    /// Overrides the protocol fee for `asset`, in basis points (max 1000 =
    /// 10%), independent of the global rate set via
    /// [`set_fee`](Self::set_fee). Admin only.
    pub fn set_asset_fee(env: Env, asset: Symbol, bps: u32) -> Result<(), Error> {
        Self::require_admin(&env)?;
        if bps > MAX_FEE_BPS {
            return Err(Error::InvalidFee);
        }
        storage::set_asset_fee(&env, &asset, bps);
        events::asset_fee_changed(&env, &asset, bps);
        Ok(())
    }

    /// Clears any fee override for `asset`, reverting it to the global fee.
    /// Admin only.
    pub fn clear_asset_fee(env: Env, asset: Symbol) -> Result<(), Error> {
        Self::require_admin(&env)?;
        storage::clear_asset_fee(&env, &asset);
        events::asset_fee_cleared(&env, &asset);
        Ok(())
    }

    /// Returns the effective protocol fee for `asset`, in basis points: its
    /// override if one is configured, otherwise the global fee.
    pub fn asset_fee(env: Env, asset: Symbol) -> u32 {
        Self::effective_fee_bps(&env, &asset)
    }

    /// Sets the number of ledgers a pending settlement may remain open before
    /// it can be reclaimed via
    /// [`cancel_expired_settlement`](Self::cancel_expired_settlement). A
    /// value of zero (the default) disables expiry entirely. Admin only.
    ///
    /// # Live-read vs. frozen-at-open
    ///
    /// Unlike the settlement fee — which is computed and stored on the
    /// [`Settlement`](crate::types::Settlement) record at
    /// [`open_settlement`](Self::open_settlement) time and never changes —
    /// the expiry window is **read live** from storage on every call to
    /// [`cancel_expired_settlement`](Self::cancel_expired_settlement) and
    /// [`is_settlement_expired`](Self::is_settlement_expired). Changing this
    /// value retroactively affects **all pending settlements**, shortening or
    /// lengthening their effective lifetime.
    pub fn set_settlement_expiry_ledgers(env: Env, ledgers: u32) -> Result<(), Error> {
        Self::require_admin(&env)?;
        storage::set_settlement_expiry_ledgers(&env, ledgers);
        events::settlement_expiry_changed(&env, ledgers);
        Ok(())
    }

    /// Returns the settlement expiry window in ledgers (zero if disabled).
    pub fn settlement_expiry_ledgers(env: Env) -> u32 {
        storage::get_settlement_expiry_ledgers(&env)
    }

    /// Returns up to `limit` currently registered anchors that hold an active
    /// fee waiver, in registration order, scanning the registration history
    /// starting at list index `start` (0-based). Mirrors
    /// [`list_anchors`](Self::list_anchors), but additionally filters out
    /// anchors that are not currently exempt from settlement fees.
    pub fn list_fee_waived_anchors(env: Env, start: u32, limit: u32) -> Vec<Address> {
        let mut out = Vec::new(&env);
        let list = storage::get_anchor_list(&env);
        let total = list.len();
        let mut idx = start;
        while idx < total && (out.len() as u32) < limit {
            let anchor = list.get(idx).unwrap();
            if storage::is_anchor(&env, &anchor) && storage::is_fee_waived(&env, &anchor) {
                out.push_back(anchor);
            }
            idx += 1;
        }
        out
    }

    /// Returns the number of currently registered anchors that have an active
    /// fee waiver. Scans the anchor list (same as
    /// [`list_fee_waived_anchors`](Self::list_fee_waived_anchors)) but
    /// returns a single count instead of a page, sparing callers from
    /// paginating just to get a total.
    pub fn fee_waived_anchor_count(env: Env) -> u32 {
        let mut count = 0;
        for anchor in storage::get_anchor_list(&env).iter() {
            if storage::is_anchor(&env, &anchor) && storage::is_fee_waived(&env, &anchor) {
                count += 1;
            }
        }
        count
    }

    /// Collects the accrued protocol fees for `asset`, resetting the balance to
    /// zero and returning the collected amount. Admin only.
    pub fn collect_fees(env: Env, asset: Symbol) -> Result<i128, Error> {
        Self::require_admin(&env)?;
        let amount = storage::get_fees_accrued(&env, &asset);
        if amount == 0 {
            return Err(Error::NoFeesToCollect);
        }
        storage::set_fees_accrued(&env, &asset, 0);
        events::fees_collected(&env, &asset, amount);
        Ok(amount)
    }

    /// Registers `anchor` as an approved liquidity provider. Admin only.
    ///
    /// Returns [`Error::AnchorAlreadyRegistered`] if the anchor is already
    /// registered.
    pub fn register_anchor(env: Env, anchor: Address) -> Result<(), Error> {
        Self::require_admin(&env)?;
        if storage::is_anchor(&env, &anchor) {
            return Err(Error::AnchorAlreadyRegistered);
        }
        storage::set_anchor(&env, &anchor);
        storage::remember_anchor(&env, &anchor);
        events::anchor_registered(&env, &anchor);
        Ok(())
    }

    /// Registers every address in `anchors` as an approved liquidity
    /// provider in a single call. Admin only.
    ///
    /// Validates the entire batch before registering anything: if any
    /// address is already registered, or the same address appears more than
    /// once in `anchors`, the whole call fails with
    /// [`Error::AnchorAlreadyRegistered`] and no anchor is registered.
    pub fn register_anchors(env: Env, anchors: Vec<Address>) -> Result<(), Error> {
        Self::require_admin(&env)?;

        let mut seen = Vec::new(&env);
        for anchor in anchors.iter() {
            if storage::is_anchor(&env, &anchor) || seen.contains(&anchor) {
                return Err(Error::AnchorAlreadyRegistered);
            }
            seen.push_back(anchor);
        }

        for anchor in anchors.iter() {
            storage::set_anchor(&env, &anchor);
            storage::remember_anchor(&env, &anchor);
            events::anchor_registered(&env, &anchor);
        }
        Ok(())
    }

    /// Returns `true` if `anchor` is a registered liquidity provider.
    pub fn is_anchor(env: Env, anchor: Address) -> bool {
        storage::is_anchor(&env, &anchor)
    }

    /// Returns up to `limit` currently registered anchors, in registration
    /// order, scanning the registration history starting at list index
    /// `start` (0-based). Anchors that have been
    /// [`deregister_anchor`](Self::deregister_anchor)ed are skipped without
    /// counting toward `limit`.
    pub fn list_anchors(env: Env, start: u32, limit: u32) -> Vec<Address> {
        let mut out = Vec::new(&env);
        let list = storage::get_anchor_list(&env);
        let total = list.len();
        let mut idx = start;
        while idx < total && (out.len() as u32) < limit {
            let anchor = list.get(idx).unwrap();
            if storage::is_anchor(&env, &anchor) {
                out.push_back(anchor);
            }
            idx += 1;
        }
        out
    }

    /// Returns the number of currently registered anchors.
    pub fn anchor_count(env: Env) -> u32 {
        let mut count = 0;
        for anchor in storage::get_anchor_list(&env).iter() {
            if storage::is_anchor(&env, &anchor) {
                count += 1;
            }
        }
        count
    }

    /// Removes `anchor` from the approved set. Admin only. Existing pool
    /// liquidity is unaffected; the anchor simply cannot open new positions.
    pub fn deregister_anchor(env: Env, anchor: Address) -> Result<(), Error> {
        Self::require_admin(&env)?;
        if !storage::is_anchor(&env, &anchor) {
            return Err(Error::AnchorNotRegistered);
        }
        storage::set_anchor_flag(&env, &anchor, false);
        events::anchor_removed(&env, &anchor);
        Ok(())
    }

    /// Provides `amount` of liquidity in `asset` from `provider`.
    ///
    /// The provider must be a registered anchor and must authorize the call.
    /// The pool's total and the provider's balance are increased by `amount`.
    pub fn provide_liquidity(
        env: Env,
        provider: Address,
        asset: Symbol,
        amount: i128,
    ) -> Result<(), Error> {
        provider.require_auth();
        Self::require_not_paused(&env)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if !storage::is_anchor(&env, &provider) {
            return Err(Error::AnchorNotRegistered);
        }
        Self::do_provide(&env, &provider, &asset, amount)?;
        Ok(())
    }

    /// Provides liquidity to multiple assets for `provider` in a single call
    /// and authorization. Every `(asset, amount)` request is validated
    /// (positive amount, no asset repeated) before any of them are applied.
    /// Fails with [`Error::DuplicateAssetInBatch`] if the same asset appears
    /// more than once; use a single combined amount instead.
    pub fn provide_liquidity_multi(
        env: Env,
        provider: Address,
        requests: Vec<(Symbol, i128)>,
    ) -> Result<(), Error> {
        provider.require_auth();
        Self::require_not_paused(&env)?;
        if requests.is_empty() {
            return Err(Error::InvalidAmount);
        }
        if !storage::is_anchor(&env, &provider) {
            return Err(Error::AnchorNotRegistered);
        }

        let mut seen = Vec::new(&env);
        for (asset, amount) in requests.iter() {
            if amount <= 0 {
                return Err(Error::InvalidAmount);
            }
            if seen.contains(&asset) {
                return Err(Error::DuplicateAssetInBatch);
            }
            seen.push_back(asset.clone());
        }

        for (asset, amount) in requests.iter() {
            Self::do_provide(&env, &provider, &asset, amount)?;
        }
        Ok(())
    }

    /// Sets the minimum liquidity floor for `asset`. Once set, any
    /// [`withdraw_liquidity`](Self::withdraw_liquidity) or
    /// [`withdraw_all_liquidity`](Self::withdraw_all_liquidity) call that
    /// would leave the pool's total below `floor` fails with
    /// [`Error::BelowMinLiquidity`]. A floor of zero (the default) disables
    /// the check entirely. Admin only.
    pub fn set_min_liquidity(env: Env, asset: Symbol, floor: i128) -> Result<(), Error> {
        Self::require_admin(&env)?;
        if floor < 0 {
            return Err(Error::InvalidAmount);
        }
        storage::set_min_liquidity(&env, &asset, floor);
        events::min_liquidity_changed(&env, &asset, floor);
        Ok(())
    }

    /// Returns the minimum liquidity floor configured for `asset` (zero if
    /// disabled).
    pub fn min_liquidity(env: Env, asset: Symbol) -> i128 {
        storage::get_min_liquidity(&env, &asset)
    }

    /// Sets the maximum amount a single [`open_settlement`](Self::open_settlement)
    /// call may reserve for `asset`. A call above this cap fails with
    /// [`Error::AboveMaxSettlementAmount`]. Zero (the default) disables the
    /// check. Admin only.
    pub fn set_max_settlement_amount(env: Env, asset: Symbol, amount: i128) -> Result<(), Error> {
        Self::require_admin(&env)?;
        if amount < 0 {
            return Err(Error::InvalidAmount);
        }
        storage::set_max_settlement_amount(&env, &asset, amount);
        events::max_settlement_amount_changed(&env, &asset, amount);
        Ok(())
    }

    /// Returns the maximum settlement amount configured for `asset` (zero if
    /// disabled).
    pub fn max_settlement_amount(env: Env, asset: Symbol) -> i128 {
        storage::get_max_settlement_amount(&env, &asset)
    }

    /// Withdraws `amount` of liquidity in `asset` back to `provider`.
    ///
    /// Requires authorization from `provider` and fails with
    /// [`Error::InsufficientLiquidity`] if the provider's balance is too low,
    /// or [`Error::BelowMinLiquidity`] if it would leave the pool below its
    /// configured [`min_liquidity`](Self::min_liquidity) floor.
    pub fn withdraw_liquidity(
        env: Env,
        provider: Address,
        asset: Symbol,
        amount: i128,
    ) -> Result<(), Error> {
        provider.require_auth();
        Self::require_not_paused(&env)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let prior = storage::get_balance(&env, &provider, &asset);
        if prior < amount {
            return Err(Error::InsufficientLiquidity);
        }
        Self::require_min_liquidity(&env, &asset, amount)?;

        Self::do_withdraw(&env, &provider, &asset, amount)?;
        Ok(())
    }

    /// Withdraws liquidity from multiple assets for `provider` in a single
    /// call and authorization. Every `(asset, amount)` request is validated
    /// (positive amount, no asset repeated, sufficient balance, and the
    /// minimum liquidity floor) before any of them are applied, so one bad
    /// entry never leaves a partial batch withdrawn. Fails with
    /// [`Error::DuplicateAssetInBatch`] if the same asset appears more than
    /// once; use a single combined amount instead.
    pub fn withdraw_liquidity_multi(
        env: Env,
        provider: Address,
        requests: Vec<(Symbol, i128)>,
    ) -> Result<(), Error> {
        provider.require_auth();
        Self::require_not_paused(&env)?;
        if requests.is_empty() {
            return Err(Error::InvalidAmount);
        }

        let mut seen = Vec::new(&env);
        for (asset, amount) in requests.iter() {
            if amount <= 0 {
                return Err(Error::InvalidAmount);
            }
            if seen.contains(&asset) {
                return Err(Error::DuplicateAssetInBatch);
            }
            seen.push_back(asset.clone());

            let prior = storage::get_balance(&env, &provider, &asset);
            if prior < amount {
                return Err(Error::InsufficientLiquidity);
            }
            Self::require_min_liquidity(&env, &asset, amount)?;
        }

        for (asset, amount) in requests.iter() {
            Self::do_withdraw(&env, &provider, &asset, amount)?;
        }
        Ok(())
    }

    /// Withdraws `provider`'s entire liquidity balance in `asset` in a single
    /// call, returning the withdrawn amount. Delegates to
    /// [`withdraw_liquidity`](Self::withdraw_liquidity) internally so that
    /// both entrypoints share an identical event-emission path — any future
    /// changes to the event shape or argument encoding in
    /// [`withdraw_liquidity`](Self::withdraw_liquidity) are automatically
    /// reflected here, keeping the off-chain indexer described in the README
    /// consistent regardless of which entrypoint a caller uses.
    /// Fails with [`Error::InsufficientLiquidity`] if the provider's balance
    /// is already zero, or [`Error::BelowMinLiquidity`] if it would leave the
    /// pool below its configured [`min_liquidity`](Self::min_liquidity)
    /// floor.
    pub fn withdraw_all_liquidity(
        env: Env,
        provider: Address,
        asset: Symbol,
    ) -> Result<i128, Error> {
        let amount = storage::get_balance(&env, &provider, &asset);
        if amount == 0 {
            return Err(Error::InsufficientLiquidity);
        }
        Self::withdraw_liquidity(env, provider, asset, amount)?;
        Ok(amount)
    }

    /// Opens a settlement that reserves `amount` of `asset` liquidity for the
    /// requesting `anchor`. The reserved amount leaves the available pool and a
    /// [`SettlementStatus::Pending`] record is created. Returns the new id.
    pub fn open_settlement(
        env: Env,
        anchor: Address,
        asset: Symbol,
        amount: i128,
    ) -> Result<u64, Error> {
        anchor.require_auth();
        Self::require_not_paused(&env)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if !storage::is_anchor(&env, &anchor) {
            return Err(Error::AnchorNotRegistered);
        }
        let cap = storage::get_max_settlement_amount(&env, &asset);
        if cap > 0 && amount > cap {
            return Err(Error::AboveMaxSettlementAmount);
        }

        let mut pool = storage::get_pool(&env, &asset);
        if pool.total < amount {
            return Err(Error::InsufficientLiquidity);
        }
        pool.total = pool.total.checked_sub(amount).ok_or(Error::Overflow)?;
        storage::set_pool(&env, &asset, &pool);

        let fee = if storage::is_fee_waived(&env, &anchor) {
            let waived_fee = Self::calculate_fee(amount, Self::effective_fee_bps(&env, &asset))?;
            let current_volume = storage::get_waived_fee_volume(&env, &asset);
            let new_volume = current_volume
                .checked_add(waived_fee)
                .ok_or(Error::Overflow)?;
            storage::set_waived_fee_volume(&env, &asset, new_volume);
            0
        } else {
            Self::calculate_fee(amount, Self::effective_fee_bps(&env, &asset))?
        };
        let id = storage::get_settlement_count(&env)
            .checked_add(1)
            .ok_or(Error::Overflow)?;
        storage::set_settlement_count(&env, id);
        storage::set_settlement(
            &env,
            &Settlement {
                id,
                anchor: anchor.clone(),
                asset: asset.clone(),
                amount,
                fee,
                status: SettlementStatus::Pending,
                opened_at: env.ledger().sequence(),
            },
        );

        events::settlement_opened(&env, id, &anchor, &asset);
        Ok(id)
    }

    /// Executes a pending settlement, accruing its fee for later collection.
    /// Admin only. The reserved liquidity is considered released to the anchor.
    pub fn execute_settlement(env: Env, id: u64) -> Result<(), Error> {
        Self::require_admin(&env)?;
        let mut settlement = storage::get_settlement(&env, id).ok_or(Error::SettlementNotFound)?;
        if settlement.status != SettlementStatus::Pending {
            return Err(Error::InvalidSettlementState);
        }

        let accrued = storage::get_fees_accrued(&env, &settlement.asset);
        let new_accrued = accrued.checked_add(settlement.fee).ok_or(Error::Overflow)?;
        storage::set_fees_accrued(&env, &settlement.asset, new_accrued);

        settlement.status = SettlementStatus::Executed;
        storage::set_settlement(&env, &settlement);

        events::settlement_executed(&env, id);
        Ok(())
    }

    /// Cancels a pending settlement and returns the reserved liquidity to the
    /// pool. Requires authorization from the settlement's anchor.
    pub fn cancel_settlement(env: Env, id: u64) -> Result<(), Error> {
        let mut settlement = storage::get_settlement(&env, id).ok_or(Error::SettlementNotFound)?;
        settlement.anchor.require_auth();
        if settlement.status != SettlementStatus::Pending {
            return Err(Error::InvalidSettlementState);
        }

        let mut pool = storage::get_pool(&env, &settlement.asset);
        pool.total = pool.total.checked_add(settlement.amount).ok_or(Error::Overflow)?;
        storage::set_pool(&env, &settlement.asset, &pool);

        settlement.status = SettlementStatus::Cancelled;
        storage::set_settlement(&env, &settlement);

        events::settlement_cancelled(&env, id);
        Ok(())
    }

    /// Reclaims the reserved liquidity of a pending settlement that has sat
    /// unexecuted past the configured
    /// [`settlement_expiry_ledgers`](Self::settlement_expiry_ledgers) window,
    /// returning it to the pool. Anyone may call this — it never moves value
    /// anywhere other than back into the shared pool it came from — so no
    /// authorization is required, allowing off-chain keepers to sweep timed
    /// out settlements. Fails with [`Error::SettlementNotExpired`] if expiry
    /// is disabled (zero) or the window has not yet elapsed, and with
    /// [`Error::InvalidSettlementState`] if the settlement is not
    /// [`SettlementStatus::Pending`].
    ///
    /// # Live-read expiry window
    ///
    /// Unlike the settlement fee — which is computed and stored on the
    /// [`Settlement`](crate::types::Settlement) record at
    /// [`open_settlement`](Self::open_settlement) time and never changes —
    /// the expiry window is **read live** from storage on every invocation.
    /// Changing [`set_settlement_expiry_ledgers`](Self::set_settlement_expiry_ledgers)
    /// after a settlement is opened retroactively affects when it becomes
    /// reclaimable. This is intentional: shortening the window serves as an
    /// emergency liquidity-recovery valve, while lengthening it prevents
    /// premature sweeps.
    pub fn cancel_expired_settlement(env: Env, id: u64) -> Result<(), Error> {
        let mut settlement = storage::get_settlement(&env, id).ok_or(Error::SettlementNotFound)?;
        if settlement.status != SettlementStatus::Pending {
            return Err(Error::InvalidSettlementState);
        }

        let expiry = storage::get_settlement_expiry_ledgers(&env);
        if expiry == 0 {
            return Err(Error::SettlementNotExpired);
        }
        let expires_at = settlement
            .opened_at
            .checked_add(expiry)
            .ok_or(Error::Overflow)?;
        if env.ledger().sequence() < expires_at {
            return Err(Error::SettlementNotExpired);
        }

        let mut pool = storage::get_pool(&env, &settlement.asset);
        pool.total = pool.total.checked_add(settlement.amount).ok_or(Error::Overflow)?;
        storage::set_pool(&env, &settlement.asset, &pool);

        settlement.status = SettlementStatus::Expired;
        storage::set_settlement(&env, &settlement);

        events::settlement_expired(&env, id);
        Ok(())
    }

    /// Returns `true` if the settlement with `id` is still
    /// [`SettlementStatus::Pending`] and has passed the configured
    /// [`settlement_expiry_ledgers`](Self::settlement_expiry_ledgers) window,
    /// i.e. is currently reclaimable via
    /// [`cancel_expired_settlement`](Self::cancel_expired_settlement).
    /// Returns `false` while expiry is disabled (zero) or the settlement is
    /// not pending. Reads only — never mutates state — so off-chain keepers
    /// can check before attempting a reclaim rather than submitting a
    /// transaction that might fail.
    pub fn is_settlement_expired(env: Env, id: u64) -> Result<bool, Error> {
        let settlement = storage::get_settlement(&env, id).ok_or(Error::SettlementNotFound)?;
        if settlement.status != SettlementStatus::Pending {
            return Ok(false);
        }
        let expiry = storage::get_settlement_expiry_ledgers(&env);
        if expiry == 0 {
            return Ok(false);
        }
        let expires_at = match settlement.opened_at.checked_add(expiry) {
            Some(v) => v,
            None => return Err(Error::Overflow),
        };
        Ok(env.ledger().sequence() >= expires_at)
    }

    /// Returns the age of the settlement with `id` in ledgers elapsed since it
    /// was opened (using the simulated ledger sequence, not wall-clock time).
    /// Returns [`Error::SettlementNotFound`] if the settlement does not exist.
    /// This is a complementary raw-value view to [`is_settlement_expired`](Self::is_settlement_expired).
    pub fn settlement_age(env: Env, id: u64) -> Result<u32, Error> {
        let settlement = storage::get_settlement(&env, id).ok_or(Error::SettlementNotFound)?;
        Ok(env.ledger().sequence() - settlement.opened_at)
    }

    /// Returns the [`Pool`] for `asset`, or [`Error::PoolNotFound`] if no
    /// liquidity has ever been provided for it.
    pub fn pool(env: Env, asset: Symbol) -> Result<Pool, Error> {
        if !storage::has_pool(&env, &asset) {
            return Err(Error::PoolNotFound);
        }
        Ok(storage::get_pool(&env, &asset))
    }

    /// Returns up to `limit` assets that have ever had liquidity provided, in
    /// first-use order, starting at list index `start` (0-based). Useful for
    /// discovering which assets to query via [`pool`](Self::pool) or
    /// [`collect_fees`](Self::collect_fees) without an off-chain indexer.
    pub fn list_assets(env: Env, start: u32, limit: u32) -> Vec<Symbol> {
        let mut out = Vec::new(&env);
        let list = storage::get_asset_list(&env);
        let total = list.len();
        let mut idx = start;
        while idx < total && (out.len() as u32) < limit {
            out.push_back(list.get(idx).unwrap());
            idx += 1;
        }
        out
    }

    /// Returns the number of distinct assets that have ever had liquidity
    /// provided. Unlike [`anchor_count`](Self::anchor_count), assets are
    /// never "deregistered" from the enumeration backing
    /// [`list_assets`](Self::list_assets), so this is simply that list's
    /// length, sparing callers from paginating through it just to count.
    pub fn asset_count(env: Env) -> u32 {
        storage::get_asset_list(&env).len()
    }

    /// Returns the total liquidity available in `asset` across all providers.
    pub fn total_liquidity(env: Env, asset: Symbol) -> i128 {
        storage::get_pool(&env, &asset).total
    }

    /// Returns the sum of [`total_liquidity`](Self::total_liquidity) across
    /// every asset that has ever had liquidity provided (per
    /// [`list_assets`](Self::list_assets)). The result mixes units across
    /// assets, so it is only meaningful as a coarse, asset-agnostic activity
    /// signal rather than a spendable amount.
    pub fn total_liquidity_all(env: Env) -> Result<i128, Error> {
        let mut total: i128 = 0;
        for asset in storage::get_asset_list(&env).iter() {
            total = total
                .checked_add(storage::get_pool(&env, &asset).total)
                .ok_or(Error::Overflow)?;
        }
        Ok(total)
    }

    /// Returns the sum of [`fees_accrued`](Self::fees_accrued) across every
    /// asset that has ever had liquidity provided, i.e. the total protocol
    /// fees outstanding across the whole contract awaiting
    /// [`collect_fees`](Self::collect_fees).
    pub fn total_fees_accrued(env: Env) -> Result<i128, Error> {
        let mut total: i128 = 0;
        for asset in storage::get_asset_list(&env).iter() {
            total = total
                .checked_add(storage::get_fees_accrued(&env, &asset))
                .ok_or(Error::Overflow)?;
        }
        Ok(total)
    }

    /// Returns the forgone protocol fee revenue for `asset` due to active waivers.
    pub fn waived_fee_volume(env: Env, asset: Symbol) -> i128 {
        storage::get_waived_fee_volume(&env, &asset)
    }

    /// Returns the sum of [`waived_fee_volume`](Self::waived_fee_volume) across every
    /// asset that has ever had liquidity provided.
    pub fn total_waived_fee_volume(env: Env) -> Result<i128, Error> {
        let mut total: i128 = 0;
        for asset in storage::get_asset_list(&env).iter() {
            total = total
                .checked_add(storage::get_waived_fee_volume(&env, &asset))
                .ok_or(Error::Overflow)?;
        }
        Ok(total)
    }

    /// Returns `provider`'s liquidity balance in `asset` (zero if none).
    pub fn balance(env: Env, provider: Address, asset: Symbol) -> i128 {
        storage::get_balance(&env, &provider, &asset)
    }

    /// Returns up to `limit` of `provider`'s non-zero balances, as
    /// `(asset, balance)` pairs, scanning
    /// [`list_assets`](Self::list_assets) starting at index `start`. Spares
    /// off-chain callers from calling [`balance`](Self::balance) once per
    /// known asset just to discover which ones a provider actually holds.
    pub fn anchor_balances(
        env: Env,
        provider: Address,
        start: u32,
        limit: u32,
    ) -> Vec<(Symbol, i128)> {
        let mut out = Vec::new(&env);
        let assets = storage::get_asset_list(&env);
        let total = assets.len();
        let mut idx = start;
        while idx < total && (out.len() as u32) < limit {
            let asset = assets.get(idx).unwrap();
            let balance = storage::get_balance(&env, &provider, &asset);
            if balance != 0 {
                out.push_back((asset, balance));
            }
            idx += 1;
        }
        out
    }

    /// Returns the settlement with `id`, or [`Error::SettlementNotFound`].
    pub fn settlement(env: Env, id: u64) -> Result<Settlement, Error> {
        storage::get_settlement(&env, id).ok_or(Error::SettlementNotFound)
    }

    /// Returns the number of settlements ever opened.
    pub fn settlement_count(env: Env) -> u64 {
        storage::get_settlement_count(&env)
    }

    /// Returns `true` if a settlement with `id` exists.
    pub fn settlement_exists(env: Env, id: u64) -> bool {
        storage::get_settlement(&env, id).is_some()
    }

    /// Returns `true` if a settlement with `id` exists and its status is
    /// [`SettlementStatus::Pending`]. Returns `false` (not an error) for a
    /// missing id or any terminal-state settlement. Designed as a minimal-payload
    /// primitive purpose-built for a keeper's hot polling path.
    ///
    /// Note the distinction from [`settlement_exists`](Self::settlement_exists)
    /// (which returns true regardless of status), [`settlement`](Self::settlement)
    /// (which errors on missing and returns the full struct), and
    /// [`is_settlement_expired`](Self::is_settlement_expired) (which returns true
    /// only if the settlement is pending AND past its expiry window).
    pub fn is_settlement_pending(env: Env, id: u64) -> bool {
        if let Some(settlement) = storage::get_settlement(&env, id) {
            settlement.status == SettlementStatus::Pending
        } else {
            false
        }
    }

    /// Returns up to `limit` settlements starting at id `start` (inclusive).
    /// Ids are assigned sequentially from 1; missing ids are skipped.
    pub fn list_settlements(env: Env, start: u64, limit: u32) -> Vec<Settlement> {
        let mut out = Vec::new(&env);
        let count = storage::get_settlement_count(&env);
        let mut id = if start == 0 { 1 } else { start };
        while id <= count && (out.len() as u32) < limit {
            if let Some(settlement) = storage::get_settlement(&env, id) {
                out.push_back(settlement);
            }
            id += 1;
        }
        out
    }

    /// Returns up to `limit` settlements opened by `anchor`, starting at id
    /// `start` (inclusive). Ids are assigned sequentially from 1; missing or
    /// non-matching ids are skipped without counting toward `limit`.
    pub fn list_settlements_by_anchor(
        env: Env,
        anchor: Address,
        start: u64,
        limit: u32,
    ) -> Vec<Settlement> {
        let mut out = Vec::new(&env);
        let count = storage::get_settlement_count(&env);
        let mut id = if start == 0 { 1 } else { start };
        while id <= count && (out.len() as u32) < limit {
            if let Some(settlement) = storage::get_settlement(&env, id) {
                if settlement.anchor == anchor {
                    out.push_back(settlement);
                }
            }
            id += 1;
        }
        out
    }

    /// Returns up to `limit` settlements in `asset`, starting at id `start`
    /// (inclusive). Ids are assigned sequentially from 1; missing or
    /// non-matching ids are skipped without counting toward `limit`.
    pub fn list_settlements_by_asset(
        env: Env,
        asset: Symbol,
        start: u64,
        limit: u32,
    ) -> Vec<Settlement> {
        let mut out = Vec::new(&env);
        let count = storage::get_settlement_count(&env);
        let mut id = if start == 0 { 1 } else { start };
        while id <= count && (out.len() as u32) < limit {
            if let Some(settlement) = storage::get_settlement(&env, id) {
                if settlement.asset == asset {
                    out.push_back(settlement);
                }
            }
            id += 1;
        }
        out
    }

    /// Returns up to `limit` settlements matching both `anchor` and `asset`,
    /// starting at id `start` (inclusive). Ids are assigned sequentially from 1;
    /// missing or non-matching ids are skipped without counting toward `limit`.
    pub fn list_settlements_by_anchor_and_asset(
        env: Env,
        anchor: Address,
        asset: Symbol,
        start: u64,
        limit: u32,
    ) -> Vec<Settlement> {
        let mut out = Vec::new(&env);
        let count = storage::get_settlement_count(&env);
        let mut id = if start == 0 { 1 } else { start };
        while id <= count && (out.len() as u32) < limit {
            if let Some(settlement) = storage::get_settlement(&env, id) {
                if settlement.anchor == anchor && settlement.asset == asset {
                    out.push_back(settlement);
                }
            }
            id += 1;
        }
        out
    }

    /// Returns up to `limit` settlements whose lifecycle state matches
    /// `status`, starting at id `start` (inclusive). Ids are assigned
    /// sequentially from 1; missing or non-matching ids are skipped without
    /// counting toward `limit`. Mirrors
    /// [`list_settlements_by_anchor`](Self::list_settlements_by_anchor) and
    /// [`list_settlements_by_asset`](Self::list_settlements_by_asset),
    /// filtering by lifecycle state instead of anchor or asset.
    pub fn list_settlements_by_status(
        env: Env,
        status: SettlementStatus,
        start: u64,
        limit: u32,
    ) -> Vec<Settlement> {
        let mut out = Vec::new(&env);
        let count = storage::get_settlement_count(&env);
        let mut id = if start == 0 { 1 } else { start };
        while id <= count && (out.len() as u32) < limit {
            if let Some(settlement) = storage::get_settlement(&env, id) {
                if settlement.status == status {
                    out.push_back(settlement);
                }
            }
            id += 1;
        }
        out
    }

    /// Returns the accrued (uncollected) protocol fees for `asset`.
    pub fn fees_accrued(env: Env, asset: Symbol) -> i128 {
        storage::get_fees_accrued(&env, &asset)
    }

    /// Returns a one-call snapshot of overall contract state (version,
    /// paused flag, global fee, anchor/asset/settlement counts), for
    /// off-chain dashboards and indexers that would otherwise need several
    /// separate calls.
    pub fn contract_info(env: Env) -> ContractInfo {
        ContractInfo {
            version: Self::version(),
            paused: storage::is_paused(&env),
            fee_bps: storage::get_fee_bps(&env),
            anchor_count: Self::anchor_count(env.clone()),
            asset_count: Self::asset_count(env.clone()),
            settlement_count: storage::get_settlement_count(&env),
        }
    }

    /// Returns the total number of settlements currently in `status`,
    /// scanning every settlement (unlike
    /// [`list_settlements_by_status`](Self::list_settlements_by_status),
    /// which pages and stops once `limit` matches are found).
    pub fn settlement_count_by_status(env: Env, status: SettlementStatus) -> u64 {
        let count = storage::get_settlement_count(&env);
        let mut total: u64 = 0;
        let mut id = 1;
        while id <= count {
            if let Some(settlement) = storage::get_settlement(&env, id) {
                if settlement.status == status {
                    total += 1;
                }
            }
            id += 1;
        }
        total
    }

    /// Returns the sum of `amount` across every settlement currently in
    /// `status`, scanning the full settlement history. Useful alongside
    /// [`settlement_count_by_status`](Self::settlement_count_by_status) for
    /// off-chain volume dashboards.
    pub fn total_settled_amount(env: Env, status: SettlementStatus) -> Result<i128, Error> {
        let count = storage::get_settlement_count(&env);
        let mut total: i128 = 0;
        let mut id = 1;
        while id <= count {
            if let Some(settlement) = storage::get_settlement(&env, id) {
                if settlement.status == status {
                    total = total
                        .checked_add(settlement.amount)
                        .ok_or(Error::Overflow)?;
                }
            }
            id = id.checked_add(1).ok_or(Error::Overflow)?;
        }
        Ok(total)
    }
}

impl AnchornetContract {
    /// Requires the call to be authorized by the current administrator.
    fn require_admin(env: &Env) -> Result<(), Error> {
        if !storage::has_admin(env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(env);
        admin.require_auth();
        Ok(())
    }

    /// Requires the call to be authorized by `caller`, who must be either the
    /// current administrator or the appointed operator. Unlike
    /// [`require_admin`](Self::require_admin), this takes an explicit caller
    /// since Soroban contracts have no implicit "sender" and the two
    /// eligible identities must be told apart before demanding a signature.
    fn require_admin_or_operator(env: &Env, caller: &Address) -> Result<(), Error> {
        if !storage::has_admin(env) {
            return Err(Error::NotInitialized);
        }
        let is_admin = *caller == storage::get_admin(env);
        let is_operator = storage::has_operator(env) && *caller == storage::get_operator(env);
        if !is_admin && !is_operator {
            return Err(Error::NotAuthorized);
        }
        caller.require_auth();
        Ok(())
    }

    /// Requires the contract to be active (not paused).
    fn require_not_paused(env: &Env) -> Result<(), Error> {
        if storage::is_paused(env) {
            return Err(Error::Paused);
        }
        Ok(())
    }

    /// Requires that withdrawing `amount` from `asset`'s pool would not leave
    /// its total below the configured minimum liquidity floor. A floor of
    /// zero (the default) always passes.
    fn require_min_liquidity(env: &Env, asset: &Symbol, amount: i128) -> Result<(), Error> {
        let floor = storage::get_min_liquidity(env, asset);
        if floor == 0 {
            return Ok(());
        }
        let pool = storage::get_pool(env, asset);
        let remaining = pool.total.checked_sub(amount).ok_or(Error::Overflow)?;
        if remaining < floor {
            return Err(Error::BelowMinLiquidity);
        }
        Ok(())
    }

    fn effective_fee_bps(env: &Env, asset: &Symbol) -> u32 {
        storage::get_asset_fee(env, asset).unwrap_or_else(|| storage::get_fee_bps(env))
    }

    fn calculate_fee(amount: i128, fee_bps: u32) -> Result<i128, Error> {
        let fee_bps = i128::from(fee_bps);
        let whole = (amount / BPS_DENOMINATOR)
            .checked_mul(fee_bps)
            .ok_or(Error::Overflow)?;
        let remainder = (amount % BPS_DENOMINATOR)
            .checked_mul(fee_bps)
            .ok_or(Error::Overflow)?
            / BPS_DENOMINATOR;

        whole.checked_add(remainder).ok_or(Error::Overflow)
    }

    fn do_provide(
        env: &Env,
        provider: &Address,
        asset: &Symbol,
        amount: i128,
    ) -> Result<(), Error> {
        let mut pool = storage::get_pool(env, asset);
        let prior = storage::get_balance(env, provider, asset);
        if prior == 0 {
            pool.providers = pool.providers.checked_add(1).ok_or(Error::Overflow)?;
        }
        pool.total = pool.total.checked_add(amount).ok_or(Error::Overflow)?;
        let new_balance = prior.checked_add(amount).ok_or(Error::Overflow)?;
        storage::set_pool(env, asset, &pool);
        storage::set_balance(env, provider, asset, new_balance);
        let is_new = storage::remember_asset(env, asset);
        events::liquidity_provided(env, provider, asset, amount);
        if is_new {
            events::asset_onboarded(env, asset);
        }
        Ok(())
    }

    fn do_withdraw(
        env: &Env,
        provider: &Address,
        asset: &Symbol,
        amount: i128,
    ) -> Result<(), Error> {
        let prior = storage::get_balance(env, provider, asset);
        let mut pool = storage::get_pool(env, asset);
        pool.total = pool.total.checked_sub(amount).ok_or(Error::Overflow)?;
        let remaining = prior.checked_sub(amount).ok_or(Error::Overflow)?;
        if remaining == 0 {
            pool.providers = pool.providers.checked_sub(1).ok_or(Error::Overflow)?;
        }
        storage::set_pool(env, asset, &pool);
        storage::set_balance(env, provider, asset, remaining);

        events::liquidity_withdrawn(env, provider, asset, amount);
        Ok(())
    }
}

#[cfg(test)]
mod test;
