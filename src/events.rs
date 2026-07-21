//! Event publishing helpers.
//!
//! Indexers (the AnchorNet backend) subscribe to these events to keep an
//! off-chain view of pool liquidity in sync with on-chain state.
//!
//! # Event-shape guarantees
//!
//! Every public entrypoint that changes pool liquidity emits a single event
//! with a deterministic topic/data shape. The two withdrawal entrypoints
//! — [`withdraw_liquidity`](crate::AnchornetContract::withdraw_liquidity) and
//! [`withdraw_all_liquidity`](crate::AnchornetContract::withdraw_all_liquidity)
//! — share the same internal emission path: `withdraw_all_liquidity` delegates
//! to `withdraw_liquidity`, which calls [`liquidity_withdrawn`]. This means
//! that for an equivalent withdrawal (same provider, asset, amount), both
//! entrypoints produce an identical event with topics
//! `("withdraw", provider, asset)` and data `amount`.
//!
//! This parity is enforced by a regression test
//! (`test_withdraw_event_parity` in `test.rs`) so that any future refactor
//! that would break the contract is caught immediately.

use soroban_sdk::{symbol_short, Address, Env, Symbol};

/// Emitted once when the contract is initialized. Topics: `("init",)`.
pub fn initialized(env: &Env, admin: &Address) {
    env.events()
        .publish((symbol_short!("init"),), admin.clone());
}

/// Emitted when the administrator changes. Topics: `("admin", path)`, where
/// `path` is `"direct"` for single-step transfers or `"accept"` for two-step proposals.
pub fn admin_changed(env: &Env, new_admin: &Address, via_proposal: bool) {
    let path = if via_proposal {
        symbol_short!("accept")
    } else {
        symbol_short!("direct")
    };
    env.events()
        .publish((symbol_short!("admin"), path), new_admin.clone());
}

/// Emitted when an admin transfer is proposed. Topics: `("propose",)`.
pub fn admin_proposed(env: &Env, candidate: &Address) {
    env.events()
        .publish((symbol_short!("propose"),), candidate.clone());
}

/// Emitted when an anchor is registered. Topics: `("anchor", anchor)`.
pub fn anchor_registered(env: &Env, anchor: &Address) {
    env.events()
        .publish((symbol_short!("anchor"), anchor.clone()), ());
}

/// Emitted when liquidity is provided. Topics: `("provide", provider, asset)`.
pub fn liquidity_provided(env: &Env, provider: &Address, asset: &Symbol, amount: i128) {
    env.events().publish(
        (symbol_short!("provide"), provider.clone(), asset.clone()),
        amount,
    );
}

/// Emitted when liquidity is withdrawn. Topics: `("withdraw", provider, asset)`, data: `amount`.
///
/// Both [`withdraw_liquidity`](crate::AnchornetContract::withdraw_liquidity)
/// and [`withdraw_all_liquidity`](crate::AnchornetContract::withdraw_all_liquidity)
/// emit this event via the same internal code path (the latter delegates to the
/// former), guaranteeing identical topic/data shape for equivalent withdrawals.
/// See the [module-level docs](self) for the full parity contract.
pub fn liquidity_withdrawn(env: &Env, provider: &Address, asset: &Symbol, amount: i128) {
    env.events().publish(
        (symbol_short!("withdraw"), provider.clone(), asset.clone()),
        amount,
    );
}

/// Emitted when the paused flag changes. Topics: `("paused",)`, data: `bool`.
pub fn paused_changed(env: &Env, paused: bool) {
    env.events().publish((symbol_short!("paused"),), paused);
}

/// Emitted when the protocol fee changes. Topics: `("fee",)`, data: `u32` bps.
pub fn fee_changed(env: &Env, bps: u32) {
    env.events().publish((symbol_short!("fee"),), bps);
}

/// Emitted when an anchor is deregistered. Topics: `("deanchor", anchor)`.
pub fn anchor_removed(env: &Env, anchor: &Address) {
    env.events()
        .publish((symbol_short!("deanchor"), anchor.clone()), ());
}

/// Emitted when a settlement is opened. Topics: `("settle", anchor, asset)`.
pub fn settlement_opened(env: &Env, id: u64, anchor: &Address, asset: &Symbol) {
    env.events()
        .publish((symbol_short!("settle"), anchor.clone(), asset.clone()), id);
}

/// Emitted when a settlement is executed. Topics: `("executed", id)`.
pub fn settlement_executed(env: &Env, id: u64) {
    env.events().publish((symbol_short!("executed"), id), ());
}

/// Emitted when a settlement is cancelled. Topics: `("cancelled", id)`.
pub fn settlement_cancelled(env: &Env, id: u64) {
    env.events().publish((symbol_short!("cancelled"), id), ());
}

/// Emitted when an anchor's fee waiver flag changes. Topics:
/// `("waiver", anchor)`, data: `bool`.
pub fn fee_waiver_changed(env: &Env, anchor: &Address, waived: bool) {
    env.events()
        .publish((symbol_short!("waiver"), anchor.clone()), waived);
}

/// Emitted when accrued fees are collected. Topics: `("collect", asset)`.
pub fn fees_collected(env: &Env, asset: &Symbol, amount: i128) {
    env.events()
        .publish((symbol_short!("collect"), asset.clone()), amount);
}

/// Emitted when the settlement expiry window changes. Topics:
/// `("expiry",)`, data: `u32` ledgers.
pub fn settlement_expiry_changed(env: &Env, ledgers: u32) {
    env.events().publish((symbol_short!("expiry"),), ledgers);
}

/// Emitted when a timed-out settlement is reclaimed. Topics:
/// `("expired", id)`.
pub fn settlement_expired(env: &Env, id: u64) {
    env.events().publish((symbol_short!("expired"), id), ());
}

/// Emitted when an asset's minimum liquidity floor changes. Topics:
/// `("minliq", asset)`, data: `i128` floor.
pub fn min_liquidity_changed(env: &Env, asset: &Symbol, floor: i128) {
    env.events()
        .publish((symbol_short!("minliq"), asset.clone()), floor);
}

/// Emitted when the operator address changes. Topics: `("operator",)`.
pub fn operator_changed(env: &Env, operator: &Address) {
    env.events()
        .publish((symbol_short!("operator"),), operator.clone());
}

/// Emitted when an asset's maximum settlement amount changes. Topics:
/// `("maxamt", asset)`, data: `i128` amount.
pub fn max_settlement_amount_changed(env: &Env, asset: &Symbol, amount: i128) {
    env.events()
        .publish((symbol_short!("maxamt"), asset.clone()), amount);
}

/// Emitted when an asset's fee override is set. Topics: `("assetfee",
/// asset)`, data: `u32` bps.
pub fn asset_fee_changed(env: &Env, asset: &Symbol, bps: u32) {
    env.events()
        .publish((symbol_short!("assetfee"), asset.clone()), bps);
}

/// Emitted when an asset's fee override is cleared, reverting it to the
/// global fee. Topics: `("feeclear", asset)`.
pub fn asset_fee_cleared(env: &Env, asset: &Symbol) {
    env.events()
        .publish((symbol_short!("feeclear"), asset.clone()), ());
}
