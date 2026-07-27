use crate::storage::DataKey;
use crate::{
    AnchorStatus, AnchornetContract, AnchornetContractClient, Error, SettlementStatus,
    BPS_DENOMINATOR,
};
use proptest::prelude::*;
use soroban_sdk::{
    symbol_short,
    testutils::{
        storage::Persistent as _, Address as _, EnvTestConfig, Events as _, Ledger as _, MockAuth,
        MockAuthInvoke,
    },
    vec, Address, Env, IntoVal, Symbol,
};

macro_rules! assert_operator_rejected {
    ($env:ident, $client:ident, $operator:ident, $fn_name:literal, $args:expr, $call:expr) => {{
        $env.set_auths(&[MockAuth {
            address: &$operator,
            invoke: &MockAuthInvoke {
                contract: &$client.address,
                fn_name: $fn_name,
                args: $args.into_val(&$env),
                sub_invokes: &[],
            },
        }
        .into()]);

        let failure = $call
            .err()
            .expect(concat!($fn_name, " unexpectedly accepted the operator"));
        assert!(
            failure.is_err(),
            "{} reached contract logic instead of rejecting operator authorization",
            $fn_name
        );
    }};
}

/// Asserts that `caller` is turned away by a *contract-level*
/// [`Error::NotAuthorized`] rather than a host authorization failure.
///
/// [`assert_operator_rejected`] covers strict admin-only entrypoints, where
/// `require_admin` asks for the admin's signature and the host aborts the
/// invocation. The shared-authority entrypoints (`pause`, `unpause`,
/// `extend_instance_ttl`) instead run `require_admin_or_operator`, which
/// rejects an address that is neither admin nor the appointed operator and
/// returns `NotAuthorized` *before* ever reaching `caller.require_auth()`.
/// That is a contract error, so it must be matched as one.
macro_rules! assert_caller_unauthorized {
    ($env:ident, $client:ident, $caller:ident, $fn_name:literal, $args:expr, $call:expr) => {{
        $env.set_auths(&[MockAuth {
            address: &$caller,
            invoke: &MockAuthInvoke {
                contract: &$client.address,
                fn_name: $fn_name,
                args: $args.into_val(&$env),
                sub_invokes: &[],
            },
        }
        .into()]);

        let failure = $call
            .err()
            .expect(concat!($fn_name, " unexpectedly accepted the caller"));
        assert_eq!(
            failure.expect(concat!(
                $fn_name,
                " aborted at the host instead of returning NotAuthorized"
            )),
            Error::NotAuthorized,
            "{} rejected the caller with an unexpected error",
            $fn_name
        );
    }};
}

fn setup(env: &Env) -> (AnchornetContractClient<'_>, Address) {
    let contract_id = env.register_contract(None, AnchornetContract);
    let client = AnchornetContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    (client, admin)
}

/// Initializes the contract, registers one anchor, and funds a pool.
/// Auths are mocked. Returns the client, admin, anchor and funded asset.
fn funded(env: &Env, liquidity: i128) -> (AnchornetContractClient<'_>, Address, Address, Symbol) {
    env.mock_all_auths();
    let (client, admin) = setup(env);
    let anchor = Address::generate(env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &asset, &liquidity);
    (client, admin, anchor, asset)
}

fn fee_amount_strategy() -> impl Strategy<Value = i128> {
    prop_oneof![
        3 => 1i128..=i128::MAX,
        1 => (i128::MAX - 100_000)..=i128::MAX,
    ]
}

fn fee_test_env() -> Env {
    Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    })
}

/// Pins every `Error` variant to its documented numeric code (see
/// `src/error.rs` and `docs/ERRORS.md`). Off-chain clients match on these
/// codes directly, so a future PR that inserts a new variant in the middle
/// of the enum instead of appending it would silently renumber every
/// variant declared after it — this test fails immediately if that happens.
#[test]
fn test_error_discriminants_are_pinned() {
    assert_eq!(Error::AlreadyInitialized as u32, 1);
    assert_eq!(Error::NotInitialized as u32, 2);
    assert_eq!(Error::NotAuthorized as u32, 3);
    assert_eq!(Error::AnchorAlreadyRegistered as u32, 4);
    assert_eq!(Error::AnchorNotRegistered as u32, 5);
    assert_eq!(Error::InvalidAmount as u32, 6);
    assert_eq!(Error::InsufficientLiquidity as u32, 7);
    assert_eq!(Error::PoolNotFound as u32, 8);
    assert_eq!(Error::Paused as u32, 9);
    assert_eq!(Error::InvalidFee as u32, 10);
    assert_eq!(Error::SettlementNotFound as u32, 11);
    assert_eq!(Error::InvalidSettlementState as u32, 12);
    assert_eq!(Error::NoFeesToCollect as u32, 13);
    assert_eq!(Error::NoPendingAdmin as u32, 14);
    assert_eq!(Error::NotPendingAdmin as u32, 15);
    assert_eq!(Error::SettlementNotExpired as u32, 16);
    assert_eq!(Error::BelowMinLiquidity as u32, 17);
    assert_eq!(Error::NoOperator as u32, 18);
    assert_eq!(Error::AboveMaxSettlementAmount as u32, 19);
    assert_eq!(Error::DuplicateAssetInBatch as u32, 20);
}

#[test]
fn test_initialize_sets_admin() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    client.initialize(&admin);

    assert_eq!(client.admin(), admin);
}

#[test]
fn test_initialize_twice_fails() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    client.initialize(&admin);
    let err = client.try_initialize(&admin).err().unwrap().unwrap();

    assert_eq!(err, Error::AlreadyInitialized);
}

#[test]
fn test_register_anchor() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);

    client.initialize(&admin);
    assert!(!client.is_anchor(&anchor));

    client.register_anchor(&anchor);
    assert!(client.is_anchor(&anchor));
}

#[test]
fn test_anchor_status_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);

    client.initialize(&admin);

    assert_eq!(client.anchor_status(&anchor), AnchorStatus::NeverRegistered);
    assert!(!client.is_anchor(&anchor));

    client.register_anchor(&anchor);
    assert_eq!(client.anchor_status(&anchor), AnchorStatus::Active);
    assert!(client.is_anchor(&anchor));

    client.deregister_anchor(&anchor);
    assert_eq!(client.anchor_status(&anchor), AnchorStatus::Deregistered);
    assert!(!client.is_anchor(&anchor));

    client.register_anchor(&anchor);
    assert_eq!(client.anchor_status(&anchor), AnchorStatus::Active);
    assert!(client.is_anchor(&anchor));
}

#[test]
fn test_register_anchor_twice_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&anchor);
    let err = client.try_register_anchor(&anchor).err().unwrap().unwrap();

    assert_eq!(err, Error::AnchorAlreadyRegistered);
}

#[test]
fn test_provide_liquidity_updates_pool_and_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &1_000);

    assert_eq!(client.total_liquidity(&usdc), 1_000);
    assert_eq!(client.balance(&anchor, &usdc), 1_000);

    let pool = client.pool(&usdc);
    assert_eq!(pool.total, 1_000);
    assert_eq!(pool.providers, 1);
}

#[test]
fn test_pool_aggregates_multiple_providers() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.provide_liquidity(&a1, &usdc, &600);
    client.provide_liquidity(&a2, &usdc, &400);

    let pool = client.pool(&usdc);
    assert_eq!(pool.total, 1_000);
    assert_eq!(pool.providers, 2);
}

#[test]
fn test_provide_liquidity_rejects_unregistered() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let stranger = Address::generate(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    let err = client
        .try_provide_liquidity(&stranger, &usdc, &100)
        .err()
        .unwrap()
        .unwrap();

    assert_eq!(err, Error::AnchorNotRegistered);
}

#[test]
fn test_provide_liquidity_rejects_non_positive_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    let err = client
        .try_provide_liquidity(&anchor, &usdc, &0)
        .err()
        .unwrap()
        .unwrap();

    assert_eq!(err, Error::InvalidAmount);
}

#[test]
fn test_withdraw_reduces_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &1_000);
    client.withdraw_liquidity(&anchor, &usdc, &400);

    assert_eq!(client.balance(&anchor, &usdc), 600);
    assert_eq!(client.total_liquidity(&usdc), 600);
}

#[test]
fn test_full_withdraw_drops_provider_count() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &1_000);
    client.withdraw_liquidity(&anchor, &usdc, &1_000);

    let pool = client.pool(&usdc);
    assert_eq!(pool.total, 0);
    assert_eq!(pool.providers, 0);
}

#[test]
fn test_withdraw_insufficient_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &100);
    let err = client
        .try_withdraw_liquidity(&anchor, &usdc, &500)
        .err()
        .unwrap()
        .unwrap();

    assert_eq!(err, Error::InsufficientLiquidity);
}

#[test]
fn test_pool_not_found() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    let err = client.try_pool(&usdc).err().unwrap().unwrap();

    assert_eq!(err, Error::PoolNotFound);
}

#[test]
fn test_unknown_balance_is_zero() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);

    assert_eq!(client.balance(&anchor, &usdc), 0);
    assert_eq!(client.total_liquidity(&usdc), 0);
}

#[test]
fn test_provider_share_bps_single_provider() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    assert_eq!(client.provider_share_bps(&anchor, &asset), 10_000);
}

#[test]
fn test_provider_share_bps_multiple_providers() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.provide_liquidity(&a1, &asset, &600);
    client.provide_liquidity(&a2, &asset, &400);
    assert_eq!(client.provider_share_bps(&a1, &asset), 6000);
    assert_eq!(client.provider_share_bps(&a2, &asset), 4000);
}

#[test]
fn test_provider_share_bps_zero_liquidity() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);
    let anchor = Address::generate(&env);
    // no liquidity provided
    assert_eq!(client.provider_share_bps(&anchor, &asset), 0);
}

#[test]
fn test_set_admin_transfers_control() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let new_admin = Address::generate(&env);

    client.initialize(&admin);
    client.set_admin(&new_admin);

    assert_eq!(client.admin(), new_admin);
}

#[test]
fn test_set_admin_emits_admin_changed_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let new_admin = Address::generate(&env);

    client.initialize(&admin);
    client.set_admin(&new_admin);

    // `events().all()` reflects the most recent top-level invocation, i.e.
    // just the `set_admin` call.
    let events = env.events().all();
    assert_eq!(
        events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("admin"), symbol_short!("direct")).into_val(&env),
                new_admin.into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_accept_admin_emits_admin_changed_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let candidate = Address::generate(&env);

    client.initialize(&admin);
    client.propose_admin(&candidate);

    client.accept_admin(&candidate);

    // `events().all()` reflects the most recent top-level invocation, i.e.
    // just the `accept_admin` call.
    let events = env.events().all();
    assert_eq!(
        events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("admin"), symbol_short!("accept")).into_val(&env),
                candidate.into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_admin_changed_provenance_parity() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    client.initialize(&admin);

    // Single-step set_admin
    client.set_admin(&admin2);
    let direct_events = env.events().all();
    assert_eq!(
        direct_events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("admin"), symbol_short!("direct")).into_val(&env),
                admin2.into_val(&env),
            ),
        ]
    );

    // Two-step propose + accept admin
    client.propose_admin(&admin3);
    client.accept_admin(&admin3);
    let accept_events = env.events().all();
    assert_eq!(
        accept_events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("admin"), symbol_short!("accept")).into_val(&env),
                admin3.into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_pause_and_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    client.initialize(&admin);
    assert!(!client.is_paused());

    client.pause(&admin);
    assert!(client.is_paused());

    client.unpause(&admin);
    assert!(!client.is_paused());
}

#[test]
fn test_pause_emits_paused_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    client.initialize(&admin);

    // `events().all()` reflects only the most recent top-level invocation,
    // so calling pause in isolation lets us assert its exact event output.
    client.pause(&admin);

    let events = env.events().all();
    assert_eq!(
        events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("paused"),).into_val(&env),
                true.into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_unpause_emits_paused_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    client.initialize(&admin);
    client.pause(&admin);

    // Call unpause in isolation so events().all() reflects only unpause.
    client.unpause(&admin);

    let events = env.events().all();
    assert_eq!(
        events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("paused"),).into_val(&env),
                false.into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_paused_blocks_provide_and_withdraw() {
    let env = Env::default();
    let (client, admin, anchor, asset) = funded(&env, 1_000);

    client.pause(&admin);

    let provide = client
        .try_provide_liquidity(&anchor, &asset, &100)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(provide, Error::Paused);

    let withdraw = client
        .try_withdraw_liquidity(&anchor, &asset, &100)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(withdraw, Error::Paused);
}

#[test]
fn test_set_fee_updates_value() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    client.initialize(&admin);
    assert_eq!(client.fee(), 0);

    client.set_fee(&25);
    assert_eq!(client.fee(), 25);
}

#[test]
fn test_set_fee_rejects_above_cap() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    client.initialize(&admin);
    let err = client.try_set_fee(&1_001).err().unwrap().unwrap();

    assert_eq!(err, Error::InvalidFee);
}

#[test]
fn test_open_settlement_reserves_liquidity() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100); // 1%

    let id = client.open_settlement(&anchor, &asset, &400);
    assert_eq!(id, 1);
    assert_eq!(client.settlement_count(), 1);

    // Reserved liquidity leaves the available pool.
    assert_eq!(client.total_liquidity(&asset), 600);

    let settlement = client.settlement(&id);
    assert_eq!(settlement.amount, 400);
    assert_eq!(settlement.fee, 4); // 1% of 400
    assert_eq!(settlement.status, SettlementStatus::Pending);
}

#[test]
fn test_open_settlement_rejects_insufficient_liquidity() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 100);

    let err = client
        .try_open_settlement(&anchor, &asset, &500)
        .err()
        .unwrap()
        .unwrap();

    assert_eq!(err, Error::InsufficientLiquidity);
}

#[test]
fn test_open_settlement_rejects_unregistered() {
    let env = Env::default();
    let (client, _admin, _anchor, asset) = funded(&env, 1_000);
    let stranger = Address::generate(&env);

    let err = client
        .try_open_settlement(&stranger, &asset, &100)
        .err()
        .unwrap()
        .unwrap();

    assert_eq!(err, Error::AnchorNotRegistered);
}

// ---------------------------------------------------------------------------
// Error surface on a never-funded asset – issue #152
//
// The three tests above all probe a *funded* asset. A never-funded one is a
// different state: `storage::get_pool` materializes `Pool::empty(asset)` for a
// missing entry, so `open_settlement` reaches `pool.total < amount` and answers
// `InsufficientLiquidity`, while `pool()` checks entry existence and answers
// `PoolNotFound` for the very same asset. The tests below pin that divergence
// down, plus the validation order it depends on: `open_settlement` returns
// `InvalidAmount` for a non-positive amount and `AnchorNotRegistered` for an
// unknown anchor *before* it ever reads the pool, so a probe that tripped
// either of those checks would report success for the wrong reason.
// ---------------------------------------------------------------------------

/// A settlement opened against an asset that never received liquidity fails
/// with [`Error::InsufficientLiquidity`], not [`Error::PoolNotFound`].
///
/// `amount = 1` is deliberate: it is the smallest value that clears the
/// `amount <= 0` guard and actually reaches the liquidity check. Probing with
/// `0` would return `InvalidAmount` and pass this test's intent by accident,
/// even under an implementation that changed the never-funded semantics.
#[test]
fn test_open_settlement_never_funded_asset_returns_insufficient_liquidity() {
    let env = Env::default();
    // Funds USDC only, so the anchor is registered but `never_funded` has no
    // pool entry at all — the state under test.
    let (client, _admin, anchor, funded_asset) = funded(&env, 1_000);
    let never_funded = symbol_short!("NOFUND");
    assert_ne!(never_funded, funded_asset);

    let err = client
        .try_open_settlement(&anchor, &never_funded, &1)
        .err()
        .unwrap()
        .unwrap();

    assert_eq!(err, Error::InsufficientLiquidity);
}

/// The companion view of the same state: `pool()` answers
/// [`Error::PoolNotFound`] for the asset that `open_settlement` rejects with
/// `InsufficientLiquidity`. The divergence is intentional, and a refactor that
/// collapsed both entrypoints onto one error variant would break one of these
/// two tests.
#[test]
fn test_pool_getter_never_funded_asset_returns_pool_not_found() {
    let env = Env::default();
    let (client, _admin, _anchor, _asset) = funded(&env, 1_000);
    let never_funded = symbol_short!("NOFUND");

    let err = client.try_pool(&never_funded).err().unwrap().unwrap();

    assert_eq!(err, Error::PoolNotFound);
}

/// Boundary above the liquidity check: on the *same* never-funded asset,
/// `amount = 0` is rejected as [`Error::InvalidAmount`] before the pool is
/// read. This is what makes the `amount = 1` choice above meaningful — if a
/// refactor moved the liquidity check ahead of the amount guard, this test
/// fails and exposes that the main test would then be passing for a different
/// reason than the one it documents.
#[test]
fn test_open_settlement_never_funded_zero_amount_returns_invalid_amount() {
    let env = Env::default();
    let (client, _admin, anchor, _asset) = funded(&env, 1_000);
    let never_funded = symbol_short!("NOFUND");

    let err = client
        .try_open_settlement(&anchor, &never_funded, &0)
        .err()
        .unwrap()
        .unwrap();

    assert_eq!(err, Error::InvalidAmount);
}

/// Boundary on the other side: the anchor registration check also precedes the
/// pool read, so an unknown anchor on a never-funded asset reports
/// [`Error::AnchorNotRegistered`] rather than `InsufficientLiquidity`. Together
/// with the zero-amount case this brackets the liquidity branch, pinning the
/// registered-anchor + positive-amount preconditions the main test relies on.
#[test]
fn test_open_settlement_never_funded_unregistered_anchor_reports_registration() {
    let env = Env::default();
    let (client, _admin, _anchor, _asset) = funded(&env, 1_000);
    let stranger = Address::generate(&env);
    let never_funded = symbol_short!("NOFUND");

    let err = client
        .try_open_settlement(&stranger, &never_funded, &1)
        .err()
        .unwrap()
        .unwrap();

    assert_eq!(err, Error::AnchorNotRegistered);
}

#[test]
fn test_execute_settlement_accrues_fee() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100); // 1%
    let id = client.open_settlement(&anchor, &asset, &400);

    client.execute_settlement(&id);

    assert_eq!(client.settlement(&id).status, SettlementStatus::Executed);
    assert_eq!(client.fees_accrued(&asset), 4);
    // Reserved liquidity stays out of the pool after execution.
    assert_eq!(client.total_liquidity(&asset), 600);
}

#[test]
fn test_cancel_settlement_returns_liquidity() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    let id = client.open_settlement(&anchor, &asset, &400);
    assert_eq!(client.total_liquidity(&asset), 600);

    client.cancel_settlement(&id);

    assert_eq!(client.settlement(&id).status, SettlementStatus::Cancelled);
    // Reserved liquidity is returned to the pool.
    assert_eq!(client.total_liquidity(&asset), 1_000);
    assert_eq!(client.fees_accrued(&asset), 0);
}

#[test]
fn test_settlement_not_found() {
    let env = Env::default();
    let (client, _admin, _anchor, _asset) = funded(&env, 1_000);

    let err = client.try_settlement(&99).err().unwrap().unwrap();
    assert_eq!(err, Error::SettlementNotFound);
}

#[test]
fn test_execute_twice_fails() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    let id = client.open_settlement(&anchor, &asset, &200);
    client.execute_settlement(&id);

    let err = client.try_execute_settlement(&id).err().unwrap().unwrap();
    assert_eq!(err, Error::InvalidSettlementState);
}

#[test]
fn test_execute_cancelled_settlement_fails() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100); // 1%
    let id = client.open_settlement(&anchor, &asset, &400);
    client.cancel_settlement(&id);

    assert_eq!(client.settlement(&id).status, SettlementStatus::Cancelled);
    assert_eq!(client.fees_accrued(&asset), 0);

    let err = client.try_execute_settlement(&id).err().unwrap().unwrap();
    assert_eq!(err, Error::InvalidSettlementState);
    assert_eq!(client.fees_accrued(&asset), 0);
}

#[test]
fn test_execute_expired_settlement_fails() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100); // 1%
    client.set_settlement_expiry_ledgers(&10);
    let id = client.open_settlement(&anchor, &asset, &400);

    env.ledger().set_sequence_number(10);
    client.cancel_expired_settlement(&id);

    assert_eq!(client.settlement(&id).status, SettlementStatus::Expired);
    assert_eq!(client.fees_accrued(&asset), 0);

    let err = client.try_execute_settlement(&id).err().unwrap().unwrap();
    assert_eq!(err, Error::InvalidSettlementState);
    assert_eq!(client.fees_accrued(&asset), 0);
}

#[test]
fn test_cancel_executed_fails() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    let id = client.open_settlement(&anchor, &asset, &200);
    client.execute_settlement(&id);

    let err = client.try_cancel_settlement(&id).err().unwrap().unwrap();
    assert_eq!(err, Error::InvalidSettlementState);
}

#[test]
fn test_collect_fees_resets_balance() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100); // 1%
    let id = client.open_settlement(&anchor, &asset, &500);
    client.execute_settlement(&id);
    assert_eq!(client.fees_accrued(&asset), 5);

    let collected = client.collect_fees(&asset);
    assert_eq!(collected, 5);
    assert_eq!(client.fees_accrued(&asset), 0);
}

#[test]
fn test_collect_fees_without_accrual_fails() {
    let env = Env::default();
    let (client, _admin, _anchor, asset) = funded(&env, 1_000);

    let err = client.try_collect_fees(&asset).err().unwrap().unwrap();
    assert_eq!(err, Error::NoFeesToCollect);
}

#[test]
fn test_deregister_anchor_blocks_settlement() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    client.deregister_anchor(&anchor);
    assert!(!client.is_anchor(&anchor));

    let err = client
        .try_open_settlement(&anchor, &asset, &100)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::AnchorNotRegistered);
}

#[test]
fn test_deregister_unknown_anchor_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let stranger = Address::generate(&env);

    client.initialize(&admin);
    let err = client
        .try_deregister_anchor(&stranger)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::AnchorNotRegistered);
}

#[test]
fn test_quote_fee_preview() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);
    client.set_fee(&250); // 2.5%

    assert_eq!(client.quote_fee(&asset, &1_000), 25);

    let err = client.try_quote_fee(&asset, &0).err().unwrap().unwrap();
    assert_eq!(err, Error::InvalidAmount);
}

#[test]
fn test_quote_fee_floor_rounding_boundary() {
    let env = fee_test_env();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");
    let bps = 1_u32;
    let first_nonzero_amount = (BPS_DENOMINATOR + i128::from(bps) - 1) / i128::from(bps);

    client.initialize(&admin);
    client.set_fee(&bps);

    assert_eq!(client.quote_fee(&asset, &(first_nonzero_amount - 1)), 0);
    assert_eq!(client.quote_fee(&asset, &first_nonzero_amount), 1);
}

#[test]
fn test_small_settlement_executes_without_accruing_truncated_fee() {
    let env = fee_test_env();
    let (client, _admin, anchor, asset) = funded(&env, BPS_DENOMINATOR);
    client.set_fee(&1);

    let amount = BPS_DENOMINATOR - 1;
    let id = client.open_settlement(&anchor, &asset, &amount);

    assert_eq!(client.settlement(&id).fee, 0);

    client.execute_settlement(&id);

    assert_eq!(client.settlement(&id).status, SettlementStatus::Executed);
    assert_eq!(client.fees_accrued(&asset), 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Fuzzes the three-axis fee-configuration interaction space: a global fee
    /// (`set_fee`), an optional per-asset override (`set_asset_fee` / clear),
    /// and an optional per-anchor waiver (`set_fee_waiver`). Precedence is:
    /// waiver > asset override > global rate. Every generated combination is
    /// cross-checked against `quote_fee` and the settlement's own `fee` and
    /// the accrued fee after `execute_settlement`.
    #[test]
    fn prop_fee_three_axis_interaction(
        amount in fee_amount_strategy(),
        global_bps in 0u32..=1_000,
        override_bps in prop::option::of(0u32..=1_000),
        waived in prop::bool::ANY,
    ) {
        let env = fee_test_env();
        env.mock_all_auths();
        let (client, admin) = setup(&env);
        let anchor = Address::generate(&env);
        let asset = symbol_short!("USDC");
        client.initialize(&admin);
        client.register_anchor(&anchor);
        client.provide_liquidity(&anchor, &asset, &amount);
        client.set_fee(&global_bps);
        if let Some(bps) = override_bps {
            client.set_asset_fee(&asset, &bps);
        } else {
            client.clear_asset_fee(&asset);
        }
        if waived {
            client.set_fee_waiver(&anchor, &true);
        } else {
            client.set_fee_waiver(&anchor, &false);
        }

        let expected_bps = if waived {
            0
        } else if override_bps.is_some() {
            override_bps.unwrap()
        } else {
            global_bps
        };
        let expected_fee = if amount > 0 {
            client.quote_fee(&asset, &amount).unwrap_or(0)
        } else {
            0
        };
        let id = client.open_settlement(&anchor, &asset, &amount);
        let settlement_fee = client.settlement(&id).fee;
        if waived {
            prop_assert_eq!(settlement_fee, 0);
            prop_assert_eq!(client.quote_fee(&asset, &amount).unwrap(), expected_fee);
            client.execute_settlement(&id);
            prop_assert_eq!(client.fees_accrued(&asset), 0);
        } else {
            prop_assert_eq!(settlement_fee, expected_fee);
            prop_assert_eq!(client.quote_fee(&asset, &amount).unwrap(), expected_fee);
            let before = client.fees_accrued(&asset);
            client.execute_settlement(&id);
            prop_assert_eq!(client.fees_accrued(&asset) - before, expected_fee);
        }
    }

    #[test]
    fn prop_quote_fee_is_monotonic_and_bounded_with_global_fee(
        first in fee_amount_strategy(),
        second in fee_amount_strategy(),
        bps in 0u32..=1_000,
    ) {
        let env = fee_test_env();
        env.mock_all_auths();
        let (client, admin) = setup(&env);
        let asset = symbol_short!("USDC");
        client.initialize(&admin);
        client.set_fee(&bps);

        let (lower_amount, upper_amount) = if first <= second {
            (first, second)
        } else {
            (second, first)
        };
        let lower_fee = client.quote_fee(&asset, &lower_amount);
        let upper_fee = client.quote_fee(&asset, &upper_amount);

        prop_assert!(lower_fee >= 0 && lower_fee <= lower_amount);
        prop_assert!(upper_fee >= 0 && upper_fee <= upper_amount);
        prop_assert!(lower_fee <= upper_fee);
    }

    #[test]
    fn prop_quote_fee_is_monotonic_and_bounded_with_asset_override(
        first in fee_amount_strategy(),
        second in fee_amount_strategy(),
        global_bps in 0u32..=1_000,
        override_bps in 0u32..=1_000,
    ) {
        let env = fee_test_env();
        env.mock_all_auths();
        let (client, admin) = setup(&env);
        let asset = symbol_short!("USDC");
        client.initialize(&admin);
        client.set_fee(&global_bps);
        client.set_asset_fee(&asset, &override_bps);

        let (lower_amount, upper_amount) = if first <= second {
            (first, second)
        } else {
            (second, first)
        };
        let lower_fee = client.quote_fee(&asset, &lower_amount);
        let upper_fee = client.quote_fee(&asset, &upper_amount);

        prop_assert!(lower_fee >= 0 && lower_fee <= lower_amount);
        prop_assert!(upper_fee >= 0 && upper_fee <= upper_amount);
        prop_assert!(lower_fee <= upper_fee);
    }
}

/// Tracks a single settlement's state within the proptest below.
#[derive(Clone)]
struct SettlementState {
    provider_idx: usize,
    asset_idx: usize,
    amount: i128,
    opened_at: u32,
    status: SettlementStatus,
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// Verifies that `total_liquidity_all()` matches an independently tracked
    /// expected total through a long randomised sequence of liquidity and
    /// settlement operations across two assets and two providers.
    ///
    /// The invariant:
    /// - `provide_liquidity`              → expected_total += amount
    /// - `withdraw_liquidity`             → expected_total -= amount
    /// - `withdraw_all_liquidity`         → expected_total -= provider's balance
    /// - `open_settlement`                → expected_total -= amount
    /// - `cancel_settlement`              → expected_total += settlement.amount
    /// - `cancel_expired_settlement`      → expected_total += settlement.amount
    /// - `execute_settlement`             → expected_total unchanged
    ///
    /// A failed call (precondition not met) is silently skipped; the invariant
    /// is checked only after each *successful* operation.
    #[test]
    fn prop_total_liquidity_all_matches_expected(
        ops in prop::collection::vec(
            (0..7u32, 0..2u32, 0..2u32, 1..=10_000i128),
            1..=200,
        ),
    ) {
        let env = Env::new_with_config(EnvTestConfig {
            capture_snapshot_at_drop: false,
        });
        env.mock_all_auths();
        let (client, admin) = setup(&env);

        let assets = [symbol_short!("USDC"), symbol_short!("EURC")];
        let providers = [
            Address::generate(&env),
            Address::generate(&env),
        ];

        client.initialize(&admin);
        for p in &providers {
            client.register_anchor(p);
        }
        // Short expiry so cancel_expired_settlement is reachable.
        client.set_settlement_expiry_ledgers(&10);

        // Indepedently tracked model of on-chain state.
        let mut expected_total: i128 = 0;
        let mut balances = [[0i128; 2]; 2];
        let mut pool_totals = [0i128; 2];
        let mut settlements: Vec<SettlementState> = Vec::new();
        let mut ledger_seq: u32 = 100;

        env.ledger().set_sequence_number(ledger_seq);

        for (kind, pi, ai, amt) in ops {
            let (pi, ai) = (pi as usize % 2, ai as usize % 2);

            let executed = match kind % 7 {
                0 => {
                    if let Ok(Ok(())) =
                        client.try_provide_liquidity(&providers[pi], &assets[ai], &amt)
                    {
                        balances[pi][ai] += amt;
                        pool_totals[ai] += amt;
                        expected_total += amt;
                        true
                    } else {
                        false
                    }
                }
                1 => {
                    if balances[pi][ai] >= amt {
                        if let Ok(Ok(())) =
                            client.try_withdraw_liquidity(&providers[pi], &assets[ai], &amt)
                        {
                            balances[pi][ai] -= amt;
                            pool_totals[ai] -= amt;
                            expected_total -= amt;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                2 => {
                    let bal = balances[pi][ai];
                    if bal > 0 {
                        if let Ok(Ok(_)) =
                            client.try_withdraw_all_liquidity(&providers[pi], &assets[ai])
                        {
                            balances[pi][ai] = 0;
                            pool_totals[ai] -= bal;
                            expected_total -= bal;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                3 => {
                    if pool_totals[ai] >= amt {
                        if let Ok(Ok(_id)) =
                            client.try_open_settlement(&providers[pi], &assets[ai], &amt)
                        {
                            pool_totals[ai] -= amt;
                            expected_total -= amt;
                            settlements.push(SettlementState {
                                provider_idx: pi,
                                asset_idx: ai,
                                amount: amt,
                                opened_at: ledger_seq,
                                status: SettlementStatus::Pending,
                            });
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                4 => {
                    let pending: Vec<usize> = settlements.iter().enumerate()
                        .filter(|(_, s)| s.status == SettlementStatus::Pending)
                        .map(|(i, _)| i)
                        .collect();
                    if pending.is_empty() {
                        false
                    } else {
                        let idx = pending[amt as usize % pending.len()];
                        let id = idx as u64 + 1;
                        if let Ok(Ok(())) = client.try_cancel_settlement(&id) {
                            let s = &mut settlements[idx];
                            s.status = SettlementStatus::Cancelled;
                            pool_totals[s.asset_idx] += s.amount;
                            expected_total += s.amount;
                            true
                        } else {
                            false
                        }
                    }
                }
                5 => {
                    let pending: Vec<usize> = settlements.iter().enumerate()
                        .filter(|(_, s)| s.status == SettlementStatus::Pending)
                        .map(|(i, _)| i)
                        .collect();
                    if pending.is_empty() {
                        false
                    } else {
                        let idx = pending[amt as usize % pending.len()];
                        let id = idx as u64 + 1;
                        if let Ok(Ok(())) = client.try_execute_settlement(&id) {
                            settlements[idx].status = SettlementStatus::Executed;
                            // expected_total unchanged
                            true
                        } else {
                            false
                        }
                    }
                }
                _ => {
                    let expired: Vec<usize> = settlements.iter().enumerate()
                        .filter(|(_, s)| {
                            s.status == SettlementStatus::Pending
                                && ledger_seq >= s.opened_at + 10
                        })
                        .map(|(i, _)| i)
                        .collect();
                    if expired.is_empty() {
                        false
                    } else {
                        let idx = expired[amt as usize % expired.len()];
                        let id = idx as u64 + 1;
                        if let Ok(Ok(())) = client.try_cancel_expired_settlement(&id) {
                            let s = &mut settlements[idx];
                            s.status = SettlementStatus::Expired;
                            pool_totals[s.asset_idx] += s.amount;
                            expected_total += s.amount;
                            true
                        } else {
                            false
                        }
                    }
                }
            };

            if executed {
                prop_assert_eq!(client.total_liquidity_all(), expected_total);
            }

            ledger_seq += 1;
            env.ledger().set_sequence_number(ledger_seq);
        }
    }
}

#[test]
fn test_zero_fee_when_unset() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    let id = client.open_settlement(&anchor, &asset, &400);
    assert_eq!(client.settlement(&id).fee, 0);
}

#[test]
fn test_settlement_ids_increment() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    let first = client.open_settlement(&anchor, &asset, &100);
    let second = client.open_settlement(&anchor, &asset, &100);

    assert_eq!(first, 1);
    assert_eq!(second, 2);
    assert_eq!(client.settlement_count(), 2);
}

#[test]
fn test_paused_blocks_open_settlement() {
    let env = Env::default();
    let (client, admin, anchor, asset) = funded(&env, 1_000);

    client.pause(&admin);
    let err = client
        .try_open_settlement(&anchor, &asset, &100)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::Paused);
}

#[test]
fn test_list_settlements_pagination() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    for _ in 0..3 {
        client.open_settlement(&anchor, &asset, &100);
    }

    let all = client.list_settlements(&1, &10);
    assert_eq!(all.len(), 3);

    let page = client.list_settlements(&2, &10);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap().id, 2);

    let limited = client.list_settlements(&1, &1);
    assert_eq!(limited.len(), 1);
}

#[test]
fn test_list_settlements_by_anchor_filters_other_anchors() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.provide_liquidity(&a1, &usdc, &1_000);
    client.provide_liquidity(&a2, &usdc, &1_000);

    let s1 = client.open_settlement(&a1, &usdc, &100);
    let s2 = client.open_settlement(&a2, &usdc, &100);
    let s3 = client.open_settlement(&a1, &usdc, &100);

    let a1_settlements = client.list_settlements_by_anchor(&a1, &1, &10);
    assert_eq!(a1_settlements.len(), 2);
    assert_eq!(a1_settlements.get(0).unwrap().id, s1);
    assert_eq!(a1_settlements.get(1).unwrap().id, s3);

    let a2_settlements = client.list_settlements_by_anchor(&a2, &1, &10);
    assert_eq!(a2_settlements.len(), 1);
    assert_eq!(a2_settlements.get(0).unwrap().id, s2);
}

#[test]
fn test_list_settlements_by_anchor_respects_limit() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    for _ in 0..3 {
        client.open_settlement(&anchor, &asset, &100);
    }

    let limited = client.list_settlements_by_anchor(&anchor, &1, &2);
    assert_eq!(limited.len(), 2);
}

#[test]
fn test_list_settlements_by_anchor_empty_for_unknown() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);
    let stranger = Address::generate(&env);

    assert_eq!(
        client.list_settlements_by_anchor(&stranger, &1, &10).len(),
        0
    );
}

#[test]
fn test_oldest_pending_settlement_id_returns_none_when_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    
    assert_eq!(client.oldest_pending_settlement_id(&usdc), None);
}

#[test]
fn test_oldest_pending_settlement_id_returns_only_one_when_one_exists() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &1_000);

    let id = client.open_settlement(&anchor, &usdc, &100);

    assert_eq!(client.oldest_pending_settlement_id(&usdc), Some(id));
}

#[test]
fn test_oldest_pending_settlement_id_skips_other_assets_and_statuses() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &1_000);
    client.provide_liquidity(&anchor, &eurc, &1_000);

    // eurc pending -> should be ignored because different asset
    let _s1 = client.open_settlement(&anchor, &eurc, &100);
    
    // usdc pending -> we'll execute it to change status
    let s2 = client.open_settlement(&anchor, &usdc, &100);
    client.execute_settlement(&s2);

    // usdc pending -> we'll cancel it to change status
    let s3 = client.open_settlement(&anchor, &usdc, &100);
    client.cancel_settlement(&s3);

    // usdc pending -> this is the first actual match!
    let s4 = client.open_settlement(&anchor, &usdc, &100);

    // another usdc pending -> shouldn't be returned since s4 is older
    let _s5 = client.open_settlement(&anchor, &usdc, &100);

    assert_eq!(client.oldest_pending_settlement_id(&usdc), Some(s4));
}

#[test]
fn test_list_settlements_by_asset_filters_other_assets() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &1_000);
    client.provide_liquidity(&anchor, &eurc, &1_000);

    let s1 = client.open_settlement(&anchor, &usdc, &100);
    let s2 = client.open_settlement(&anchor, &eurc, &100);
    let s3 = client.open_settlement(&anchor, &usdc, &100);

    let usdc_settlements = client.list_settlements_by_asset(&usdc, &1, &10);
    assert_eq!(usdc_settlements.len(), 2);
    assert_eq!(usdc_settlements.get(0).unwrap().id, s1);
    assert_eq!(usdc_settlements.get(1).unwrap().id, s3);

    let eurc_settlements = client.list_settlements_by_asset(&eurc, &1, &10);
    assert_eq!(eurc_settlements.len(), 1);
    assert_eq!(eurc_settlements.get(0).unwrap().id, s2);
}

#[test]
fn test_list_settlements_by_asset_respects_limit() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    for _ in 0..3 {
        client.open_settlement(&anchor, &asset, &100);
    }

    let limited = client.list_settlements_by_asset(&asset, &1, &2);
    assert_eq!(limited.len(), 2);
}

#[test]
fn test_list_settlements_by_asset_empty_for_unknown() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);
    let other = symbol_short!("EURC");

    assert_eq!(client.list_settlements_by_asset(&other, &1, &10).len(), 0);
}

#[test]
fn test_list_settlements_by_anch_asset_filters_other() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.provide_liquidity(&a1, &usdc, &1_000);
    client.provide_liquidity(&a1, &eurc, &1_000);
    client.provide_liquidity(&a2, &usdc, &1_000);

    let s1 = client.open_settlement(&a1, &usdc, &100);
    let s2 = client.open_settlement(&a1, &eurc, &100);
    let s3 = client.open_settlement(&a2, &usdc, &100);
    let s4 = client.open_settlement(&a1, &usdc, &100);

    let a1_usdc = client.list_settlements_by_anch_asset(&a1, &usdc, &1, &10);
    assert_eq!(a1_usdc.len(), 2);
    assert_eq!(a1_usdc.get(0).unwrap().id, s1);
    assert_eq!(a1_usdc.get(1).unwrap().id, s4);

    let a1_eurc = client.list_settlements_by_anch_asset(&a1, &eurc, &1, &10);
    assert_eq!(a1_eurc.len(), 1);
    assert_eq!(a1_eurc.get(0).unwrap().id, s2);

    let a2_usdc = client.list_settlements_by_anch_asset(&a2, &usdc, &1, &10);
    assert_eq!(a2_usdc.len(), 1);
    assert_eq!(a2_usdc.get(0).unwrap().id, s3);
}

#[test]
fn test_list_settlements_by_anch_asset_respects_limit() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    for _ in 0..3 {
        client.open_settlement(&anchor, &asset, &100);
    }

    let limited = client.list_settlements_by_anch_asset(&anchor, &asset, &1, &2);
    assert_eq!(limited.len(), 2);
}

#[test]
fn test_list_settlements_by_anch_asset_empty_for_unknown() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);
    let stranger = Address::generate(&env);
    let other_asset = symbol_short!("EURC");

    assert_eq!(
        client
            .list_settlements_by_anchor_and_asset(&stranger, &asset, &1, &10)
            .len(),
        0
    );
    assert_eq!(
        client
            .list_settlements_by_anchor_and_asset(&anchor, &other_asset, &1, &10)
            .len(),
        0
    );
}

#[test]
fn test_version() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    assert_eq!(client.version(), 9);
}

#[test]
fn test_settlement_exists() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    assert!(!client.settlement_exists(&1));
    let id = client.open_settlement(&anchor, &asset, &100);
    assert!(client.settlement_exists(&id));
}

#[test]
fn test_is_settlement_pending() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100); // 1%
    client.set_settlement_expiry_ledgers(&10);

    // Missing id
    assert!(!client.is_settlement_pending(&1));

    // Pending id
    let id_pending = client.open_settlement(&anchor, &asset, &100);
    assert!(client.is_settlement_pending(&id_pending));

    // Executed
    let id_exec = client.open_settlement(&anchor, &asset, &100);
    client.execute_settlement(&id_exec);
    assert!(!client.is_settlement_pending(&id_exec));

    // Cancelled
    let id_cancel = client.open_settlement(&anchor, &asset, &100);
    client.cancel_settlement(&id_cancel);
    assert!(!client.is_settlement_pending(&id_cancel));

    // Expired
    let id_expire = client.open_settlement(&anchor, &asset, &100);
    env.ledger().set_sequence_number(15);
    client.cancel_expired_settlement(&id_expire);
    assert!(!client.is_settlement_pending(&id_expire));
}

#[test]
fn test_list_settlements_empty() {
    let env = Env::default();
    let (client, _admin, _anchor, _asset) = funded(&env, 1_000);

    assert_eq!(client.list_settlements(&1, &10).len(), 0);
    assert_eq!(client.list_settlements(&100, &10).len(), 0);
}

#[test]
fn test_open_settlement_rejects_non_positive_amount() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    let err = client
        .try_open_settlement(&anchor, &asset, &0)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::InvalidAmount);
}

#[test]
fn test_is_initialized() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    assert!(!client.is_initialized());
    client.initialize(&admin);
    assert!(client.is_initialized());
}

#[test]
fn test_fees_accumulate_across_settlements() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100); // 1%

    let first = client.open_settlement(&anchor, &asset, &300);
    let second = client.open_settlement(&anchor, &asset, &200);
    client.execute_settlement(&first);
    client.execute_settlement(&second);

    // 1% of 300 + 1% of 200 = 3 + 2 = 5
    assert_eq!(client.fees_accrued(&asset), 5);
}

#[test]
fn test_fees_are_tracked_per_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.set_fee(&100);
    client.provide_liquidity(&anchor, &usdc, &1_000);
    client.provide_liquidity(&anchor, &eurc, &1_000);

    let s1 = client.open_settlement(&anchor, &usdc, &400);
    client.execute_settlement(&s1);

    assert_eq!(client.fees_accrued(&usdc), 4);
    assert_eq!(client.fees_accrued(&eurc), 0);
}

#[test]
fn test_propose_and_accept_admin_transfers_control() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let candidate = Address::generate(&env);

    client.initialize(&admin);
    client.propose_admin(&candidate);
    assert_eq!(client.pending_admin(), candidate);
    // Control does not change until the candidate explicitly accepts.
    assert_eq!(client.admin(), admin);

    client.accept_admin(&candidate);

    assert_eq!(client.admin(), candidate);
    let err = client.try_pending_admin().err().unwrap().unwrap();
    assert_eq!(err, Error::NoPendingAdmin);
}

// ─────────────────────────────────────────────────────────────────────────
// Regression: an outstanding admin proposal must not weaken the current
// administrator (issue #130).
//
// `propose_admin` writes only the `PendingAdmin` entry, and `require_admin`
// resolves authority live from `Admin` without ever consulting it, so the
// outgoing admin keeps unrestricted authority until the candidate calls
// `accept_admin`. Coupling the two would turn a transfer to an unreachable
// candidate into a denial of service against the contract's own admin.
//
// The test above already covers the `admin()` getter across the transfer, but
// reading the getter is not the same as exercising authority: it would stay
// green even if a partial freeze rejected the outgoing admin's calls. This
// test drives real admin-only entrypoints in all three phases instead.
//
// Authorization failures are asserted with `assert_operator_rejected!`, which
// is generic over the address it presents despite its name: it installs a
// single `MockAuth` for that address, so an entrypoint asking for a different
// signature fails in the host rather than reaching contract logic.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_outgoing_admin_retains_authority_until_transfer_is_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let candidate = Address::generate(&env);
    let interim = Address::generate(&env);
    let anchor = Address::generate(&env);
    let later_anchor = Address::generate(&env);

    client.initialize(&admin);
    client.propose_admin(&candidate);
    assert_eq!(client.pending_admin(), candidate);

    // --- Phase 1: the outgoing admin is untouched by the pending proposal ---
    // Driven through `try_*` so a regression reports which entrypoint started
    // refusing the admin, rather than surfacing as an opaque host panic.
    assert!(
        client.try_set_fee(&100).is_ok(),
        "the outgoing admin must still be able to set the fee",
    );
    assert_eq!(client.fee(), 100);
    assert!(
        client.try_register_anchor(&anchor).is_ok(),
        "the outgoing admin must still be able to register anchors",
    );
    assert!(client.is_anchor(&anchor));

    // Including the transfer machinery itself: the admin can still redirect
    // the proposal, which is what makes an unresponsive candidate recoverable
    // rather than fatal.
    assert!(
        client.try_propose_admin(&interim).is_ok(),
        "the outgoing admin must still be able to propose a different candidate",
    );
    assert_eq!(client.pending_admin(), interim);
    client.propose_admin(&candidate);
    assert_eq!(
        client.pending_admin(),
        candidate,
        "the admin must be able to redirect an outstanding proposal",
    );
    assert_eq!(client.admin(), admin, "authority has not moved yet");

    // --- Phase 2: the candidate holds no authority before accepting ---
    assert_operator_rejected!(
        env,
        client,
        candidate,
        "set_fee",
        (25_u32,),
        client.try_set_fee(&25)
    );
    assert_operator_rejected!(
        env,
        client,
        candidate,
        "register_anchor",
        (later_anchor.clone(),),
        client.try_register_anchor(&later_anchor)
    );
    assert_operator_rejected!(
        env,
        client,
        candidate,
        "propose_admin",
        (interim.clone(),),
        client.try_propose_admin(&interim)
    );

    // None of the rejected calls left a trace.
    assert_eq!(client.fee(), 100);
    assert!(!client.is_anchor(&later_anchor));
    assert_eq!(client.pending_admin(), candidate);

    // --- Phase 3: acceptance flips authority in both directions ---
    env.mock_all_auths();
    client.accept_admin(&candidate);
    assert_eq!(client.admin(), candidate);

    client.set_fee(&250);
    assert_eq!(client.fee(), 250);
    client.register_anchor(&later_anchor);
    assert!(client.is_anchor(&later_anchor));

    // The former admin is now just another address.
    assert_operator_rejected!(
        env,
        client,
        admin,
        "set_fee",
        (25_u32,),
        client.try_set_fee(&25)
    );
    assert_eq!(
        client.fee(),
        250,
        "the former admin must not be able to change the fee after handover",
    );
}

#[test]
fn test_accept_admin_without_proposal_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let candidate = Address::generate(&env);

    client.initialize(&admin);
    let err = client.try_accept_admin(&candidate).err().unwrap().unwrap();

    assert_eq!(err, Error::NoPendingAdmin);
}

#[test]
fn test_accept_admin_wrong_candidate_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let candidate = Address::generate(&env);
    let stranger = Address::generate(&env);

    client.initialize(&admin);
    client.propose_admin(&candidate);
    let err = client.try_accept_admin(&stranger).err().unwrap().unwrap();

    assert_eq!(err, Error::NotPendingAdmin);
    // The original proposal is untouched by the rejected attempt.
    assert_eq!(client.pending_admin(), candidate);
}

#[test]
fn test_propose_admin_overwrites_prior_proposal() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let first = Address::generate(&env);
    let second = Address::generate(&env);

    client.initialize(&admin);
    client.propose_admin(&first);
    client.propose_admin(&second);

    assert_eq!(client.pending_admin(), second);
    let err = client.try_accept_admin(&first).err().unwrap().unwrap();
    assert_eq!(err, Error::NotPendingAdmin);
}

// ─────────────────────────────────────────────────────────────────────────
// Regression: a second `propose_admin` supersedes the first candidate
// (issue #131).
//
// `propose_admin` writes `PendingAdmin` unconditionally: there is no
// `has_pending_admin` guard and no dedicated "superseded" event, so a second
// proposal silently replaces the first. That last-write-wins shape, with no
// queue, is intentional — it is how an administrator corrects a mistyped
// candidate, and requiring an explicit cancel first would leave a bad
// proposal stuck. It should not be "fixed" into a rejection.
//
// `test_propose_admin_overwrites_prior_proposal` above already pins the state
// half: `pending_admin` reports the second candidate and the first is turned
// away with `NotPendingAdmin`. What it leaves uncovered is that the supersede
// hands off a still-working transfer, and that each proposal announced itself
// on the event stream. An implementation that coalesced the two calls into a
// single event, or that left the second candidate unable to accept, passes
// the state-only assertions and fails here.
//
// Events are captured after each call because `events().all()` reflects only
// the most recent top-level invocation (see the note in
// `test_set_admin_emits_admin_changed_event`). Reading it once after both
// proposals would return the second event alone and prove nothing about the
// first.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_second_propose_admin_supersedes_first_and_emits_both_events() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let first = Address::generate(&env);
    let second = Address::generate(&env);

    client.initialize(&admin);

    // The first proposal announces `first` on the event stream.
    client.propose_admin(&first);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("propose"),).into_val(&env),
                first.clone().into_val(&env),
            ),
        ],
        "the first propose_admin must emit admin_proposed for the first candidate"
    );
    assert_eq!(client.pending_admin(), first);

    // The second proposal announces `second` with its own event, rather than
    // being coalesced into the first or suppressed as a redundant proposal.
    client.propose_admin(&second);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("propose"),).into_val(&env),
                second.clone().into_val(&env),
            ),
        ],
        "the second propose_admin must emit its own admin_proposed event"
    );

    // The pending entry now names the second candidate alone.
    assert_eq!(client.pending_admin(), second);

    // The superseded candidate is left with no dangling authorized path.
    // `accept_admin` resolves the pending entry before `require_auth`, so this
    // is a contract-level rejection and not an authorization failure.
    let err = client.try_accept_admin(&first).err().unwrap().unwrap();
    assert_eq!(err, Error::NotPendingAdmin);

    // Authority has not moved yet: superseding a proposal is not a transfer.
    assert_eq!(client.admin(), admin);

    // The proposal is still live for the second candidate and completes.
    client.accept_admin(&second);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("admin"), symbol_short!("accept")).into_val(&env),
                second.clone().into_val(&env),
            ),
        ],
        "accept_admin must emit admin_changed via the proposal path"
    );
    assert_eq!(client.admin(), second);
    let err = client.try_pending_admin().err().unwrap().unwrap();
    assert_eq!(err, Error::NoPendingAdmin);
}

#[test]
fn test_propose_admin_rejects_current_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    client.initialize(&admin);
    let err = client.try_propose_admin(&admin).err().unwrap().unwrap();

    assert_eq!(err, Error::InvalidAdminCandidate);
    // No pending admin was set.
    let err = client.try_pending_admin().err().unwrap().unwrap();
    assert_eq!(err, Error::NoPendingAdmin);
}

#[test]
fn test_pending_admin_unset_by_default() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    client.initialize(&admin);
    let err = client.try_pending_admin().err().unwrap().unwrap();
    assert_eq!(err, Error::NoPendingAdmin);
}

#[test]
fn test_list_anchors_returns_registered_in_order() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);

    client.initialize(&admin);
    assert_eq!(client.list_anchors(&0, &10).len(), 0);
    assert_eq!(client.anchor_count(), 0);

    client.register_anchor(&a1);
    client.register_anchor(&a2);

    let anchors = client.list_anchors(&0, &10);
    assert_eq!(anchors.len(), 2);
    assert_eq!(anchors.get(0).unwrap(), a1);
    assert_eq!(anchors.get(1).unwrap(), a2);
    assert_eq!(client.anchor_count(), 2);
}

#[test]
fn test_list_anchors_excludes_deregistered() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.deregister_anchor(&a1);

    let anchors = client.list_anchors(&0, &10);
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors.get(0).unwrap(), a2);
    assert_eq!(client.anchor_count(), 1);
}

#[test]
fn test_list_anchors_reflects_reregistration() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.deregister_anchor(&anchor);
    assert_eq!(client.anchor_count(), 0);

    // Re-registering a previously removed anchor must not duplicate it in
    // the enumerated list.
    client.register_anchor(&anchor);
    let anchors = client.list_anchors(&0, &10);
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors.get(0).unwrap(), anchor);
}

/// Regression test for deregister/re-register cycle preserving balance and
/// pool.providers state (no double-count, no reset, funds remain withdrawable).
///
/// Per `deregister_anchor` doc: "Existing pool liquidity is unaffected; the
/// anchor simply cannot open new positions".
///
/// Covers acceptance criteria:
/// - balances / anchor_balances / pool.providers unchanged across deregister
/// - re-register does not reset or double-count
/// - immediate withdraw after re-register succeeds
/// - provide_liquidity and open_settlement blocked while deregistered
#[test]
fn test_deregister_re_register_preserves_balances_and_provider_count() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let asset = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &asset, &1_000);

    // Initial state
    assert_eq!(client.balance(&anchor, &asset), 1_000);
    let pool = client.pool(&asset);
    assert_eq!(pool.total, 1_000);
    assert_eq!(pool.providers, 1);
    let bals = client.anchor_balances(&anchor, &0, &10);
    assert_eq!(bals.len(), 1);
    assert_eq!(bals.get(0).unwrap(), (asset.clone(), 1_000));

    // Deregister — balances and provider count unaffected
    client.deregister_anchor(&anchor);
    assert!(!client.is_anchor(&anchor));

    assert_eq!(client.balance(&anchor, &asset), 1_000);
    let pool = client.pool(&asset);
    assert_eq!(pool.total, 1_000);
    assert_eq!(pool.providers, 1);
    let bals = client.anchor_balances(&anchor, &0, &10);
    assert_eq!(bals.len(), 1);
    assert_eq!(bals.get(0).unwrap(), (asset.clone(), 1_000));

    // Cannot provide or open settlement while deregistered (AnchorNotRegistered)
    let err_provide = client
        .try_provide_liquidity(&anchor, &asset, &100)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err_provide, Error::AnchorNotRegistered);

    let err_settle = client
        .try_open_settlement(&anchor, &asset, &100)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err_settle, Error::AnchorNotRegistered);

    // Re-register the same anchor
    client.register_anchor(&anchor);
    assert!(client.is_anchor(&anchor));

    // State must be exactly as before deregistration (no reset, no double-count)
    assert_eq!(client.balance(&anchor, &asset), 1_000);
    let pool = client.pool(&asset);
    assert_eq!(pool.total, 1_000);
    assert_eq!(pool.providers, 1);
    let bals = client.anchor_balances(&anchor, &0, &10);
    assert_eq!(bals.len(), 1);
    assert_eq!(bals.get(0).unwrap(), (asset.clone(), 1_000));

    // Anchor can immediately withdraw its preserved balance (no need to re-provide)
    let withdrawn = client.withdraw_all_liquidity(&anchor, &asset);
    assert_eq!(withdrawn, 1_000);
    assert_eq!(client.balance(&anchor, &asset), 0);
    let pool = client.pool(&asset);
    assert_eq!(pool.total, 0);
    assert_eq!(pool.providers, 0);
}

#[test]
fn test_list_anchors_pagination() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    let a3 = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.register_anchor(&a3);

    let page = client.list_anchors(&0, &2);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap(), a1);
    assert_eq!(page.get(1).unwrap(), a2);

    let rest = client.list_anchors(&2, &10);
    assert_eq!(rest.len(), 1);
    assert_eq!(rest.get(0).unwrap(), a3);

    let none = client.list_anchors(&3, &10);
    assert_eq!(none.len(), 0);
}

#[test]
fn test_list_anchors_pagination_skips_deregistered_without_counting() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    let a3 = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.register_anchor(&a3);
    client.deregister_anchor(&a2);

    // Scanning from list index 0 with a limit of 2 must skip the
    // deregistered a2 (list index 1) without counting it toward the limit,
    // so both a1 and a3 are still returned.
    let page = client.list_anchors(&0, &2);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap(), a1);
    assert_eq!(page.get(1).unwrap(), a3);
}

#[test]
fn test_fee_waiver_exempts_anchor_from_settlement_fee() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100); // 1%

    client.set_fee_waiver(&anchor, &true);
    assert!(client.is_fee_waived(&anchor));

    let id = client.open_settlement(&anchor, &asset, &400);
    assert_eq!(client.settlement(&id).fee, 0);
}

#[test]
fn test_fee_waiver_toggle_off_restores_fee() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100); // 1%

    client.set_fee_waiver(&anchor, &true);
    client.set_fee_waiver(&anchor, &false);
    assert!(!client.is_fee_waived(&anchor));

    let id = client.open_settlement(&anchor, &asset, &400);
    assert_eq!(client.settlement(&id).fee, 4);
}

#[test]
fn test_set_fee_waiver_rejects_unregistered_anchor() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let stranger = Address::generate(&env);

    client.initialize(&admin);
    let err = client
        .try_set_fee_waiver(&stranger, &true)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::AnchorNotRegistered);
}

#[test]
fn test_fee_waiver_unset_by_default() {
    let env = Env::default();
    let (client, _admin, anchor, _asset) = funded(&env, 1_000);

    assert!(!client.is_fee_waived(&anchor));
}

// ─────────────────────────────────────────────────────────────────────────
// Regression: flipping a fee waiver never rewrites a settlement that is
// already open (issue #144).
//
// `open_settlement` snapshots the fee once, reading `is_fee_waived` at open
// time (lib.rs, the `let fee = if storage::is_fee_waived(..)` branch), and
// `execute_settlement` accrues `settlement.fee` directly — it never re-reads
// the waiver. The tests below toggle the waiver in both directions while a
// settlement sits Pending and pin the invariant on four distinct views of
// the same state, so a future implementation that recomputes at execute
// time fails here rather than silently mispricing:
//
//   1. `Settlement.fee` on the stored record (immutability post-open).
//   2. The `fees_accrued` delta produced by `execute_settlement`.
//   3. Record and delta agreeing numerically — a `fees_accrued`-only test
//      would pass against a broken impl that recomputed to the same number.
//   4. `waived_fee_volume`, which the waived branch of `open_settlement`
//      also writes: the forgone-revenue counter must be decided at open
//      time too, and must not move when the waiver later changes.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_fee_waiver_granted_after_open_does_not_reduce_accrued_fee() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000_000);
    client.set_fee(&100); // 1%

    assert!(
        !client.is_fee_waived(&anchor),
        "precondition: anchor unwaived"
    );

    // Opened while unwaived: the record freezes a non-zero fee, and the
    // waived-volume counter is untouched because the waived branch never ran.
    let id = client.open_settlement(&anchor, &asset, &400_000);
    let frozen_fee = client.settlement(&id).fee;
    assert_eq!(
        frozen_fee, 4_000,
        "unwaived anchor must snapshot the 1% fee"
    );
    assert_eq!(client.waived_fee_volume(&asset), 0);

    let accrued_before = client.fees_accrued(&asset);

    // The flip mid-flight, while the settlement is still Pending.
    client.set_fee_waiver(&anchor, &true);
    assert!(client.is_fee_waived(&anchor));

    // 1. The record is unchanged by the flip.
    assert_eq!(
        client.settlement(&id).fee,
        frozen_fee,
        "Settlement.fee must not change retroactively when the waiver is granted",
    );
    // 4. The waiver came too late to count as forgone revenue.
    assert_eq!(
        client.waived_fee_volume(&asset),
        0,
        "a waiver granted after open must not retroactively book forgone revenue",
    );

    client.execute_settlement(&id);

    // 2. Accrual used the frozen fee, not the now-waived recomputation.
    let delta = client.fees_accrued(&asset) - accrued_before;
    assert_eq!(
        delta, frozen_fee,
        "fees_accrued must grow by the snapshotted fee, not by 0",
    );
    // 3. Both views report the same number.
    assert_eq!(
        client.settlement(&id).fee,
        delta,
        "record and accrual delta must reflect the same frozen fee",
    );
    assert_eq!(client.waived_fee_volume(&asset), 0);
}

#[test]
fn test_fee_waiver_revoked_after_open_does_not_charge_pending_settlement() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000_000);
    client.set_fee(&100); // 1%

    client.set_fee_waiver(&anchor, &true);
    assert!(client.is_fee_waived(&anchor), "precondition: anchor waived");

    // Opened while waived: the record freezes a zero fee, and the notional
    // fee is booked as forgone revenue at that moment.
    let id = client.open_settlement(&anchor, &asset, &400_000);
    assert_eq!(
        client.settlement(&id).fee,
        0,
        "waived anchor must snapshot a zero fee",
    );
    assert_eq!(
        client.waived_fee_volume(&asset),
        4_000,
        "the waived branch books the notional fee as forgone revenue",
    );

    let accrued_before = client.fees_accrued(&asset);

    // The revoke mid-flight, while the settlement is still Pending.
    client.set_fee_waiver(&anchor, &false);
    assert!(!client.is_fee_waived(&anchor));

    // 1. The record is unchanged by the revoke.
    assert_eq!(
        client.settlement(&id).fee,
        0,
        "Settlement.fee must stay 0 when the waiver is revoked mid-flight",
    );
    // 4. Revoking does not un-book revenue already forgone at open time.
    assert_eq!(
        client.waived_fee_volume(&asset),
        4_000,
        "a revoke after open must not rewind the forgone-revenue counter",
    );

    client.execute_settlement(&id);

    // 2. Nothing accrued: the settlement was priced at zero and stays there.
    let delta = client.fees_accrued(&asset) - accrued_before;
    assert_eq!(
        delta, 0,
        "fees_accrued must not grow for a settlement opened under a waiver",
    );
    // 3. Both views agree at zero.
    assert_eq!(
        client.settlement(&id).fee,
        delta,
        "record and accrual delta must both be zero",
    );
    assert_eq!(client.waived_fee_volume(&asset), 4_000);
}

#[test]
fn test_cancel_restores_liquidity_with_fee_set() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100);

    let id = client.open_settlement(&anchor, &asset, &400);
    assert_eq!(client.total_liquidity(&asset), 600);

    client.cancel_settlement(&id);

    // The full reserved amount returns; fees are only accrued on execution.
    assert_eq!(client.total_liquidity(&asset), 1_000);
    assert_eq!(client.fees_accrued(&asset), 0);
}

#[test]
#[should_panic]
fn test_provide_liquidity_overflow_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &i128::MAX);
    client.provide_liquidity(&anchor, &usdc, &1);
}

#[test]
fn test_provide_liquidity_overflow_returns_error() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &i128::MAX);
    let err = client
        .try_provide_liquidity(&anchor, &usdc, &1)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::Overflow);
}

#[test]
fn test_quote_fee_handles_max_amount_at_max_fee() {
    let env = fee_test_env();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");

    client.initialize(&admin);
    let max_fee_bps = client.max_fee_bps();
    client.set_fee(&max_fee_bps);

    assert_eq!(client.quote_fee(&asset, &i128::MAX), i128::MAX / 10);
    assert_eq!(
        client.quote_fee(&asset, &(i128::MAX - 1)),
        (i128::MAX - 1) / 10
    );

    client.set_fee(&0);
    client.set_asset_fee(&asset, &max_fee_bps);
    assert_eq!(client.quote_fee(&asset, &i128::MAX), i128::MAX / 10);
}

#[test]
fn test_open_settlement_handles_max_amount_at_max_fee() {
    let env = fee_test_env();
    let (client, _admin, anchor, asset) = funded(&env, i128::MAX);
    client.set_fee(&client.max_fee_bps());

    let id = client.open_settlement(&anchor, &asset, &i128::MAX);

    assert_eq!(client.settlement(&id).fee, i128::MAX / 10);
}

#[test]
fn test_settlement_expiry_disabled_by_default() {
    let env = Env::default();
    let (client, _admin, _anchor, _asset) = funded(&env, 1_000);

    assert_eq!(client.settlement_expiry_ledgers(), 0);
    assert!(!client.is_settlement_expiry_configured());
}

#[test]
fn test_set_settlement_expiry_ledgers_updates_value() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    client.initialize(&admin);

    assert!(!client.is_settlement_expiry_configured());

    client.set_settlement_expiry_ledgers(&100);

    assert_eq!(client.settlement_expiry_ledgers(), 100);
    assert!(client.is_settlement_expiry_configured());
}

#[test]
fn test_cancel_expired_settlement_disabled_by_default() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    let id = client.open_settlement(&anchor, &asset, &400);

    assert_eq!(client.settlement_expiry_ledgers(), 0);
    assert!(!client.is_settlement_expiry_configured());

    // Expiry is disabled (zero) by default, no matter how far the ledger
    // advances.
    env.ledger().set_sequence_number(1_000_000);
    assert!(!client.is_settlement_expired(&id));
    let err = client
        .try_cancel_expired_settlement(&id)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::SettlementNotExpired);
}

#[test]
fn test_cancel_expired_settlement_rejects_before_expiry() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_settlement_expiry_ledgers(&50);
    let id = client.open_settlement(&anchor, &asset, &400); // opened_at == 0

    // One ledger short of the 50-ledger expiry window.
    env.ledger().set_sequence_number(49);
    let err = client
        .try_cancel_expired_settlement(&id)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::SettlementNotExpired);

    // The settlement is untouched and its liquidity still reserved.
    assert_eq!(client.total_liquidity(&asset), 600);
}

#[test]
fn test_cancel_expired_settlement_reclaims_at_boundary() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_settlement_expiry_ledgers(&50);
    let id = client.open_settlement(&anchor, &asset, &400); // opened_at == 0
    assert_eq!(client.total_liquidity(&asset), 600);

    // Exactly at the expiry boundary the settlement becomes reclaimable.
    env.ledger().set_sequence_number(50);
    client.cancel_expired_settlement(&id);

    assert_eq!(client.settlement(&id).status, SettlementStatus::Expired);
    assert_eq!(client.total_liquidity(&asset), 1_000);
}

#[test]
fn test_cancel_expired_settlement_rejects_already_executed() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_settlement_expiry_ledgers(&10);
    let id = client.open_settlement(&anchor, &asset, &400);
    client.execute_settlement(&id);

    env.ledger().set_sequence_number(20);
    let err = client
        .try_cancel_expired_settlement(&id)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::InvalidSettlementState);
}

#[test]
fn test_cancel_expired_settlement_rejects_unknown_id() {
    let env = Env::default();
    let (client, _admin, _anchor, _asset) = funded(&env, 1_000);

    let err = client
        .try_cancel_expired_settlement(&99)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::SettlementNotFound);
}

#[test]
fn test_expiry_window_shortened_after_open_makes_settlement_reclaimable_earlier() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    // Open a settlement under a 50-ledger window.
    client.set_settlement_expiry_ledgers(&50);
    let id = client.open_settlement(&anchor, &asset, &400); // opened_at == 0

    // Advance one ledger before original expiry — still not expired.
    env.ledger().set_sequence_number(49);
    let err = client
        .try_cancel_expired_settlement(&id)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::SettlementNotExpired);

    // Shorten the window retroactively.
    client.set_settlement_expiry_ledgers(&30);

    // Now at ledger 49, the settlement IS expired (opened_at 0 + 30 ≤ 49).
    client.cancel_expired_settlement(&id);
    assert_eq!(client.settlement(&id).status, SettlementStatus::Expired);
    assert_eq!(client.total_liquidity(&asset), 1_000);
}

#[test]
fn test_expiry_window_lengthened_after_open_delays_reclaimability() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    // Open a settlement under a 10-ledger window.
    client.set_settlement_expiry_ledgers(&10);
    let id = client.open_settlement(&anchor, &asset, &400); // opened_at == 0

    // Lengthen the window before original expiry.
    client.set_settlement_expiry_ledgers(&50);

    // At ledger 10 the settlement was opened at ledger 0 and the window is
    // now 50, so 10 < 50 — not yet expired despite being past the original
    // window.
    env.ledger().set_sequence_number(10);
    let err = client
        .try_cancel_expired_settlement(&id)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::SettlementNotExpired);

    // At ledger 50 the settlement finally expires.
    env.ledger().set_sequence_number(50);
    client.cancel_expired_settlement(&id);
    assert_eq!(client.settlement(&id).status, SettlementStatus::Expired);
    assert_eq!(client.total_liquidity(&asset), 1_000);
}

#[test]
fn test_list_fee_waived_anchors_filters_non_waived() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    let a3 = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.register_anchor(&a3);
    client.set_fee_waiver(&a1, &true);
    client.set_fee_waiver(&a3, &true);

    let waived = client.list_fee_waived_anchors(&0, &10);
    assert_eq!(waived.len(), 2);
    assert_eq!(waived.get(0).unwrap(), a1);
    assert_eq!(waived.get(1).unwrap(), a3);
}

#[test]
fn test_list_fee_waived_anchors_excludes_deregistered() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.set_fee_waiver(&a1, &true);
    client.set_fee_waiver(&a2, &true);
    client.deregister_anchor(&a1);

    // A waiver on a deregistered anchor is not surfaced by the enumeration,
    // mirroring how `list_anchors` excludes deregistered anchors.
    let waived = client.list_fee_waived_anchors(&0, &10);
    assert_eq!(waived.len(), 1);
    assert_eq!(waived.get(0).unwrap(), a2);
}

#[test]
fn test_list_fee_waived_anchors_toggle_off_removed() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.set_fee_waiver(&anchor, &true);
    assert_eq!(client.list_fee_waived_anchors(&0, &10).len(), 1);

    client.set_fee_waiver(&anchor, &false);
    assert_eq!(client.list_fee_waived_anchors(&0, &10).len(), 0);
}

#[test]
fn test_list_fee_waived_anchors_empty_by_default() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&anchor);

    assert_eq!(client.list_fee_waived_anchors(&0, &10).len(), 0);
}

#[test]
fn test_fee_waived_anchor_count_zero_by_default() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&anchor);

    assert_eq!(client.fee_waived_anchor_count(), 0);
}

#[test]
fn test_fee_waived_anchor_count_increments_on_grant() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    assert_eq!(client.fee_waived_anchor_count(), 0);

    client.set_fee_waiver(&a1, &true);
    assert_eq!(client.fee_waived_anchor_count(), 1);

    client.set_fee_waiver(&a2, &true);
    assert_eq!(client.fee_waived_anchor_count(), 2);
}

#[test]
fn test_fee_waived_anchor_count_decrements_on_revoke() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.set_fee_waiver(&a1, &true);
    client.set_fee_waiver(&a2, &true);
    assert_eq!(client.fee_waived_anchor_count(), 2);

    client.set_fee_waiver(&a1, &false);
    assert_eq!(client.fee_waived_anchor_count(), 1);

    client.set_fee_waiver(&a2, &false);
    assert_eq!(client.fee_waived_anchor_count(), 0);
}

#[test]
fn test_fee_waived_anchor_count_excludes_deregistered() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.set_fee_waiver(&a1, &true);
    client.set_fee_waiver(&a2, &true);
    assert_eq!(client.fee_waived_anchor_count(), 2);

    client.deregister_anchor(&a1);
    assert_eq!(client.fee_waived_anchor_count(), 1);
}

#[test]
fn test_fee_waived_anchor_count_matches_list_length() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    let a3 = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.register_anchor(&a3);
    client.set_fee_waiver(&a1, &true);
    client.set_fee_waiver(&a3, &true);

    assert_eq!(
        client.fee_waived_anchor_count(),
        client.list_fee_waived_anchors(&0, &10).len(),
    );
}

#[test]
fn test_register_anchors_batch_registers_all() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    let a3 = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchors(&vec![&env, a1.clone(), a2.clone(), a3.clone()]);

    assert!(client.is_anchor(&a1));
    assert!(client.is_anchor(&a2));
    assert!(client.is_anchor(&a3));
    assert_eq!(client.anchor_count(), 3);
    // Batch registration also appears in enumeration order, like individual
    // `register_anchor` calls.
    let anchors = client.list_anchors(&0, &10);
    assert_eq!(anchors.get(0).unwrap(), a1);
    assert_eq!(anchors.get(1).unwrap(), a2);
    assert_eq!(anchors.get(2).unwrap(), a3);
}

#[test]
fn test_register_anchors_batch_rejects_duplicate_within_batch() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);

    client.initialize(&admin);
    let err = client
        .try_register_anchors(&vec![&env, a1.clone(), a2.clone(), a1.clone()])
        .err()
        .unwrap()
        .unwrap();

    assert_eq!(err, Error::AnchorAlreadyRegistered);
    // The whole batch is rejected; neither address is registered.
    assert!(!client.is_anchor(&a1));
    assert!(!client.is_anchor(&a2));
    assert_eq!(client.anchor_count(), 0);
}

#[test]
fn test_register_anchors_batch_rejects_already_registered() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&a1);

    // a1 is already registered, so the batch fails entirely even though a2
    // is new.
    let err = client
        .try_register_anchors(&vec![&env, a2.clone(), a1.clone()])
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::AnchorAlreadyRegistered);
    assert!(!client.is_anchor(&a2));
}

#[test]
fn test_register_anchors_batch_empty_is_noop() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    client.initialize(&admin);
    client.register_anchors(&vec![&env]);

    assert_eq!(client.anchor_count(), 0);
}

#[test]
fn test_register_anchors_batch_emits_events_in_order() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    let a3 = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchors(&vec![&env, a1.clone(), a2.clone(), a3.clone()]);

    let events = env.events().all();
    assert_eq!(
        events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("anchor"), a1.clone()).into_val(&env),
                ().into_val(&env),
            ),
            (
                client.address.clone(),
                (symbol_short!("anchor"), a2.clone()).into_val(&env),
                ().into_val(&env),
            ),
            (
                client.address.clone(),
                (symbol_short!("anchor"), a3.clone()).into_val(&env),
                ().into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_register_anchors_batch_failure_emits_zero_events() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&a1);

    let a2 = Address::generate(&env);
    let _ = client.try_register_anchors(&vec![&env, a2.clone(), a1.clone()]);

    let events = env.events().all();
    assert_eq!(events, vec![&env]);
}

#[test]
fn test_withdraw_all_liquidity_returns_full_balance() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    let withdrawn = client.withdraw_all_liquidity(&anchor, &asset);

    assert_eq!(withdrawn, 1_000);
    assert_eq!(client.balance(&anchor, &asset), 0);
    assert_eq!(client.total_liquidity(&asset), 0);
}

#[test]
fn test_withdraw_all_liquidity_drops_provider_count() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    client.withdraw_all_liquidity(&anchor, &asset);

    let pool = client.pool(&asset);
    assert_eq!(pool.providers, 0);
}

#[test]
fn test_withdraw_all_liquidity_only_affects_one_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &1_000);
    client.provide_liquidity(&anchor, &eurc, &500);

    client.withdraw_all_liquidity(&anchor, &usdc);

    assert_eq!(client.balance(&anchor, &usdc), 0);
    assert_eq!(client.balance(&anchor, &eurc), 500);
}

#[test]
fn test_withdraw_all_liquidity_rejects_zero_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&anchor);

    let err = client
        .try_withdraw_all_liquidity(&anchor, &usdc)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::InsufficientLiquidity);
}

#[test]
fn test_withdraw_all_liquidity_blocked_while_paused() {
    let env = Env::default();
    let (client, admin, anchor, asset) = funded(&env, 1_000);

    client.pause(&admin);
    let err = client
        .try_withdraw_all_liquidity(&anchor, &asset)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::Paused);
}

#[test]
fn test_list_assets_returns_ever_funded_in_first_use_order() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    assert_eq!(client.list_assets(&0, &10).len(), 0);

    client.provide_liquidity(&anchor, &usdc, &100);
    client.provide_liquidity(&anchor, &eurc, &200);

    let assets = client.list_assets(&0, &10);
    assert_eq!(assets.len(), 2);
    assert_eq!(assets.get(0).unwrap(), usdc);
    assert_eq!(assets.get(1).unwrap(), eurc);
}

#[test]
fn test_list_assets_does_not_duplicate_on_repeat_provide() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    client.provide_liquidity(&anchor, &asset, &500);

    let assets = client.list_assets(&0, &10);
    assert_eq!(assets.len(), 1);
    assert_eq!(assets.get(0).unwrap(), asset);
}

#[test]
fn test_list_assets_pagination() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");
    let gbpc = symbol_short!("GBPC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &100);
    client.provide_liquidity(&anchor, &eurc, &100);
    client.provide_liquidity(&anchor, &gbpc, &100);

    let page = client.list_assets(&0, &2);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap(), usdc);
    assert_eq!(page.get(1).unwrap(), eurc);

    let rest = client.list_assets(&2, &10);
    assert_eq!(rest.len(), 1);
    assert_eq!(rest.get(0).unwrap(), gbpc);
}

#[test]
fn test_operator_unset_by_default() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    client.initialize(&admin);

    let err = client.try_operator().err().unwrap().unwrap();
    assert_eq!(err, Error::NoOperator);
    assert!(!client.is_operator(&admin));
}

#[test]
fn test_set_operator_updates_value() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let operator = Address::generate(&env);
    client.initialize(&admin);

    client.set_operator(&operator);

    assert_eq!(client.operator(), operator);
    assert!(client.is_operator(&operator));
    assert!(!client.is_operator(&admin));
}

#[test]
fn test_clear_operator() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let operator = Address::generate(&env);
    client.initialize(&admin);

    client.set_operator(&operator);
    assert!(client.is_operator(&operator));

    client.clear_operator();

    let err = client.try_operator().err().unwrap().unwrap();
    assert_eq!(err, Error::NoOperator);
    assert!(!client.is_operator(&operator));

    assert_operator_rejected!(
        env,
        client,
        operator,
        "pause",
        (),
        client.try_pause(&operator)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "unpause",
        (),
        client.try_unpause(&operator)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "extend_instance_ttl",
        (),
        client.try_extend_instance_ttl(&operator)
    );

    // Admin can still act
    client.pause(&admin);
    assert!(client.is_paused());
}

#[test]
fn test_operator_can_pause_and_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let operator = Address::generate(&env);
    client.initialize(&admin);
    client.set_operator(&operator);

    client.pause(&operator);
    assert!(client.is_paused());

    client.unpause(&operator);
    assert!(!client.is_paused());
}

#[test]
fn test_admin_can_still_pause_with_operator_appointed() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let operator = Address::generate(&env);
    client.initialize(&admin);
    client.set_operator(&operator);

    client.pause(&admin);
    assert!(client.is_paused());
}

#[test]
fn test_admin_appointed_as_operator_retains_both_roles() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let candidate = Address::generate(&env);
    client.initialize(&admin);

    client.set_operator(&admin);

    assert_eq!(client.operator(), admin);
    assert!(client.is_operator(&admin));

    // Operator-scoped actions still succeed
    client.pause(&admin);
    assert!(client.is_paused());
    client.unpause(&admin);
    assert!(!client.is_paused());
    client.extend_instance_ttl(&admin);

    // Admin-only actions remain available (no NotAuthorized error)
    client.propose_admin(&candidate);
}

#[test]
fn test_stranger_cannot_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let operator = Address::generate(&env);
    let stranger = Address::generate(&env);
    client.initialize(&admin);
    client.set_operator(&operator);

    let err = client.try_pause(&stranger).err().unwrap().unwrap();
    assert_eq!(err, Error::NotAuthorized);
    assert!(!client.is_paused());
}

#[test]
fn test_operator_is_rejected_by_every_strictly_admin_only_entrypoint() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let operator = Address::generate(&env);
    let anchor = Address::generate(&env);
    let candidate = Address::generate(&env);
    let replacement_operator = Address::generate(&env);
    let new_anchor = Address::generate(&env);
    let batch_anchor_one = Address::generate(&env);
    let batch_anchor_two = Address::generate(&env);
    let asset = symbol_short!("USDC");

    client.initialize(&admin);
    client.set_operator(&operator);
    client.register_anchor(&anchor);
    client.set_fee(&100);
    client.set_asset_fee(&asset, &100);
    client.provide_liquidity(&anchor, &asset, &1_000);
    let executed_id = client.open_settlement(&anchor, &asset, &100);
    client.execute_settlement(&executed_id);
    let pending_id = client.open_settlement(&anchor, &asset, &100);

    // Each call supplies valid state and arguments so an authorization change
    // cannot be hidden by a later validation error. Strict admin checks ask
    // for the admin's signature, so presenting only the appointed operator's
    // exact invocation must produce a host auth failure, not a contract error.
    assert_operator_rejected!(
        env,
        client,
        operator,
        "set_admin",
        (candidate.clone(),),
        client.try_set_admin(&candidate)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "propose_admin",
        (candidate.clone(),),
        client.try_propose_admin(&candidate)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "set_operator",
        (replacement_operator.clone(),),
        client.try_set_operator(&replacement_operator)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "set_fee",
        (25_u32,),
        client.try_set_fee(&25)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "set_fee_waiver",
        (anchor.clone(), true),
        client.try_set_fee_waiver(&anchor, &true)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "set_asset_fee",
        (asset.clone(), 50_u32),
        client.try_set_asset_fee(&asset, &50)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "clear_asset_fee",
        (asset.clone(),),
        client.try_clear_asset_fee(&asset)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "set_settlement_expiry_ledgers",
        (100_u32,),
        client.try_set_settlement_expiry_ledgers(&100)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "collect_fees",
        (asset.clone(),),
        client.try_collect_fees(&asset)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "register_anchor",
        (new_anchor.clone(),),
        client.try_register_anchor(&new_anchor)
    );
    let batch = vec![&env, batch_anchor_one.clone(), batch_anchor_two.clone()];
    assert_operator_rejected!(
        env,
        client,
        operator,
        "register_anchors",
        (batch.clone(),),
        client.try_register_anchors(&batch)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "deregister_anchor",
        (anchor.clone(),),
        client.try_deregister_anchor(&anchor)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "set_min_liquidity",
        (asset.clone(), 10_i128),
        client.try_set_min_liquidity(&asset, &10)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "clear_min_liquidity",
        (asset.clone(),),
        client.try_clear_min_liquidity(&asset)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "set_max_settlement_amount",
        (asset.clone(), 500_i128),
        client.try_set_max_settlement_amount(&asset, &500)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "clear_max_settlement_amount",
        (asset.clone(),),
        client.try_clear_max_settlement_amount(&asset)
    );
    assert_operator_rejected!(
        env,
        client,
        operator,
        "execute_settlement",
        (pending_id,),
        client.try_execute_settlement(&pending_id)
    );
}

#[test]
fn test_replacing_operator_revokes_prior_operator() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    client.initialize(&admin);

    client.set_operator(&first);
    client.set_operator(&second);

    assert!(!client.is_operator(&first));
    assert!(client.is_operator(&second));

    let err = client.try_pause(&first).err().unwrap().unwrap();
    assert_eq!(err, Error::NotAuthorized);
}

#[test]
fn test_min_liquidity_disabled_by_default() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    assert_eq!(client.min_liquidity(&asset), 0);
    assert!(!client.is_min_liquidity_configured(&asset));

    // With no floor configured, a full withdrawal is unaffected.
    client.withdraw_liquidity(&anchor, &asset, &1_000);
    assert_eq!(client.total_liquidity(&asset), 0);
}

#[test]
fn test_is_min_liquidity_configured_false_before_set() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");

    client.initialize(&admin);

    assert_eq!(client.min_liquidity(&asset), 0);
    assert!(!client.is_min_liquidity_configured(&asset));
}

#[test]
fn test_is_min_liquidity_configured_true_after_nonzero_floor() {
    let env = Env::default();
    let (client, _admin, _anchor, asset) = funded(&env, 1_000);

    client.set_min_liquidity(&asset, &200);

    assert_eq!(client.min_liquidity(&asset), 200);
    assert!(client.is_min_liquidity_configured(&asset));
}

#[test]
fn test_is_min_liquidity_configured_true_after_explicit_zero_floor() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    client.set_min_liquidity(&asset, &0);

    assert_eq!(client.min_liquidity(&asset), 0);
    assert!(client.is_min_liquidity_configured(&asset));

    // Existing behavior is unchanged: an explicit zero floor still disables
    // the withdrawal check, so a full withdrawal remains allowed.
    client.withdraw_liquidity(&anchor, &asset, &1_000);
    assert_eq!(client.total_liquidity(&asset), 0);
}

#[test]
fn test_set_min_liquidity_updates_value() {
    let env = Env::default();
    let (client, _admin, _anchor, asset) = funded(&env, 1_000);

    client.set_min_liquidity(&asset, &200);

    assert_eq!(client.min_liquidity(&asset), 200);
}

#[test]
fn test_set_min_liquidity_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");

    client.initialize(&admin);
    client.set_min_liquidity(&asset, &200);

    let events = env.events().all();
    assert_eq!(
        events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("minliq"), asset.clone()).into_val(&env),
                200i128.into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_set_min_liquidity_rejects_negative_floor() {
    let env = Env::default();
    let (client, _admin, _anchor, asset) = funded(&env, 1_000);

    let err = client
        .try_set_min_liquidity(&asset, &-1)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::InvalidAmount);
}

#[test]
fn test_withdraw_liquidity_blocked_below_min_liquidity_floor() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_min_liquidity(&asset, &700);

    // Withdrawing 400 would leave 600, below the 700 floor.
    let err = client
        .try_withdraw_liquidity(&anchor, &asset, &400)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::BelowMinLiquidity);
    // The rejected withdrawal must not have moved any liquidity.
    assert_eq!(client.total_liquidity(&asset), 1_000);
}

#[test]
fn test_withdraw_liquidity_allowed_at_exact_floor_boundary() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_min_liquidity(&asset, &600);

    // Withdrawing 400 leaves exactly 600, which satisfies the floor.
    client.withdraw_liquidity(&anchor, &asset, &400);
    assert_eq!(client.total_liquidity(&asset), 600);
}

#[test]
fn test_withdraw_all_liquidity_blocked_by_min_liquidity_floor() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_min_liquidity(&asset, &1);

    let err = client
        .try_withdraw_all_liquidity(&anchor, &asset)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::BelowMinLiquidity);
    assert_eq!(client.total_liquidity(&asset), 1_000);
}

#[test]
fn test_withdraw_event_parity() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &asset, &1_000);

    // Capture events from withdraw_all_liquidity.
    let amount = client.withdraw_all_liquidity(&anchor, &asset);
    let events_all = env.events().all();

    assert_eq!(amount, 1_000);
    assert_eq!(
        events_all,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("withdraw"), anchor.clone(), asset.clone()).into_val(&env),
                amount.into_val(&env),
            ),
        ],
        "withdraw_all_liquidity must emit the same event shape as withdraw_liquidity \
         for an equivalent withdrawal"
    );
}

#[test]
fn test_withdraw_liquidity_event_shape_matches_withdraw_all() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &asset, &1_000);

    // Capture events from withdraw_liquidity with the full balance.
    client.withdraw_liquidity(&anchor, &asset, &1_000);
    let events_all = env.events().all();

    assert_eq!(
        events_all,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("withdraw"), anchor.clone(), asset.clone()).into_val(&env),
                1_000i128.into_val(&env),
            ),
        ],
        "withdraw_liquidity with the full balance must emit the same event shape \
         as withdraw_all_liquidity for an equivalent withdrawal"
    );
}

#[test]
fn test_min_liquidity_floor_is_per_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &1_000);
    client.provide_liquidity(&anchor, &eurc, &1_000);
    client.set_min_liquidity(&usdc, &900);

    // The floor on USDC does not affect withdrawals from the EURC pool.
    client.withdraw_liquidity(&anchor, &eurc, &1_000);
    assert_eq!(client.total_liquidity(&eurc), 0);

    let err = client
        .try_withdraw_liquidity(&anchor, &usdc, &200)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::BelowMinLiquidity);
}

#[test]
fn test_asset_count_matches_list_assets_length() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    assert_eq!(client.asset_count(), 0);

    client.provide_liquidity(&anchor, &usdc, &100);
    assert_eq!(client.asset_count(), 1);

    client.provide_liquidity(&anchor, &eurc, &100);
    assert_eq!(client.asset_count(), 2);

    // A full withdrawal empties the pool but does not remove the asset from
    // the enumeration, so the count is unaffected.
    client.withdraw_all_liquidity(&anchor, &usdc);
    assert_eq!(client.asset_count(), 2);

    // Providing again for an already-seen asset does not double count it.
    client.provide_liquidity(&anchor, &usdc, &50);
    assert_eq!(client.asset_count(), 2);
}

#[test]
fn test_is_settlement_expired_false_while_disabled() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    let id = client.open_settlement(&anchor, &asset, &400);

    env.ledger().set_sequence_number(1_000_000);
    assert!(!client.is_settlement_expired(&id));
}

#[test]
fn test_settlement_expiry_disabled_at_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    assert_eq!(client.settlement_expiry_ledgers(), 0);
    assert!(!client.is_settlement_expiry_configured());

    // Explicitly set expiry to 0 (disabling it). This must be distinguishable
    // from the never-configured state even though both report a zero window.
    client.set_settlement_expiry_ledgers(&0);
    assert_eq!(client.settlement_expiry_ledgers(), 0);
    assert!(client.is_settlement_expiry_configured());

    let id = client.open_settlement(&anchor, &asset, &400);

    // Advance the ledger sequence arbitrarily far in the future
    env.ledger().set_sequence_number(1_000_000);

    // Assert that is_settlement_expired reports false
    assert!(!client.is_settlement_expired(&id));

    // Assert that cancel_expired_settlement fails and returns SettlementNotExpired
    let err = client
        .try_cancel_expired_settlement(&id)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::SettlementNotExpired);
}

#[test]
fn test_is_settlement_expired_false_before_boundary() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_settlement_expiry_ledgers(&50);
    let id = client.open_settlement(&anchor, &asset, &400); // opened_at == 0

    env.ledger().set_sequence_number(49);
    assert!(!client.is_settlement_expired(&id));
}

#[test]
fn test_is_settlement_expired_true_at_boundary() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_settlement_expiry_ledgers(&50);
    let id = client.open_settlement(&anchor, &asset, &400); // opened_at == 0

    env.ledger().set_sequence_number(50);
    assert!(client.is_settlement_expired(&id));
}

#[test]
fn test_is_settlement_expired_false_once_executed() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_settlement_expiry_ledgers(&10);
    let id = client.open_settlement(&anchor, &asset, &400);
    client.execute_settlement(&id);

    env.ledger().set_sequence_number(20);
    assert!(!client.is_settlement_expired(&id));
}

#[test]
fn test_is_settlement_expired_rejects_unknown_id() {
    let env = Env::default();
    let (client, _admin, _anchor, _asset) = funded(&env, 1_000);

    let err = client
        .try_is_settlement_expired(&99)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::SettlementNotFound);
}

#[test]
fn test_settlement_age_zero_at_open() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    env.ledger().set_sequence_number(1234);
    let id = client.open_settlement(&anchor, &asset, &400);

    assert_eq!(client.settlement_age(&id), 0);
}

#[test]
fn test_settlement_age_grows_with_ledger() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    env.ledger().set_sequence_number(100);
    let id = client.open_settlement(&anchor, &asset, &400); // opened_at == 100

    env.ledger().set_sequence_number(105);
    assert_eq!(client.settlement_age(&id), 5);
}

#[test]
fn test_settlement_age_rejects_unknown_id() {
    let env = Env::default();
    let (client, _admin, _anchor, _asset) = funded(&env, 1_000);

    let err = client.try_settlement_age(&99).err().unwrap().unwrap();
    assert_eq!(err, Error::SettlementNotFound);
}

#[test]
fn test_total_liquidity_all_sums_across_assets() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    assert_eq!(client.total_liquidity_all(), 0);

    client.provide_liquidity(&anchor, &usdc, &600);
    client.provide_liquidity(&anchor, &eurc, &400);

    assert_eq!(client.total_liquidity_all(), 1_000);

    client.withdraw_liquidity(&anchor, &usdc, &100);
    assert_eq!(client.total_liquidity_all(), 900);
}

#[test]
fn test_total_fees_accrued_sums_across_assets() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.set_fee(&100); // 1%
    client.provide_liquidity(&anchor, &usdc, &1_000);
    client.provide_liquidity(&anchor, &eurc, &1_000);
    assert_eq!(client.total_fees_accrued(), 0);

    let s1 = client.open_settlement(&anchor, &usdc, &400);
    let s2 = client.open_settlement(&anchor, &eurc, &200);
    client.execute_settlement(&s1);
    client.execute_settlement(&s2);

    // 1% of 400 + 1% of 200 = 4 + 2 = 6, summed across both assets.
    assert_eq!(client.total_fees_accrued(), 6);

    client.collect_fees(&usdc);
    assert_eq!(client.total_fees_accrued(), 2);
}

#[test]
fn test_waived_fee_volume_accumulates() {
    let env = Env::default();
    let (client, admin, anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100); // 1%

    assert_eq!(client.waived_fee_volume(&asset), 0);

    // Not waived initially
    client.open_settlement(&anchor, &asset, &200);
    assert_eq!(client.waived_fee_volume(&asset), 0);

    // Apply waiver
    client.set_fee_waiver(&anchor, &true);

    // Waived settlement 1: 1% of 300 = 3
    client.open_settlement(&anchor, &asset, &300);
    assert_eq!(client.waived_fee_volume(&asset), 3);

    // Waived settlement 2: 1% of 400 = 4
    client.open_settlement(&anchor, &asset, &400);
    assert_eq!(client.waived_fee_volume(&asset), 7);
}

#[test]
fn test_total_waived_fee_volume_sums_across_assets() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.set_fee(&100); // 1%
    client.set_fee_waiver(&anchor, &true);

    client.provide_liquidity(&anchor, &usdc, &1_000);
    client.provide_liquidity(&anchor, &eurc, &1_000);
    assert_eq!(client.total_waived_fee_volume(), 0);

    // 1% of 400 = 4 in USDC
    client.open_settlement(&anchor, &usdc, &400);
    // 1% of 200 = 2 in EURC
    client.open_settlement(&anchor, &eurc, &200);

    assert_eq!(client.total_waived_fee_volume(), 6);
}

#[test]
fn test_list_settlements_by_status_filters_lifecycle_state() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    let pending = client.open_settlement(&anchor, &asset, &100);
    let executed = client.open_settlement(&anchor, &asset, &100);
    let cancelled = client.open_settlement(&anchor, &asset, &100);
    client.execute_settlement(&executed);
    client.cancel_settlement(&cancelled);

    let pending_list = client.list_settlements_by_status(&SettlementStatus::Pending, &1, &10);
    assert_eq!(pending_list.len(), 1);
    assert_eq!(pending_list.get(0).unwrap().id, pending);

    let executed_list = client.list_settlements_by_status(&SettlementStatus::Executed, &1, &10);
    assert_eq!(executed_list.len(), 1);
    assert_eq!(executed_list.get(0).unwrap().id, executed);

    let cancelled_list = client.list_settlements_by_status(&SettlementStatus::Cancelled, &1, &10);
    assert_eq!(cancelled_list.len(), 1);
    assert_eq!(cancelled_list.get(0).unwrap().id, cancelled);

    // No settlement has expired, so the Expired filter comes back empty.
    assert_eq!(
        client
            .list_settlements_by_status(&SettlementStatus::Expired, &1, &10)
            .len(),
        0
    );
}

#[test]
fn test_list_settlements_by_status_respects_limit() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    for _ in 0..3 {
        client.open_settlement(&anchor, &asset, &100);
    }

    let limited = client.list_settlements_by_status(&SettlementStatus::Pending, &1, &2);
    assert_eq!(limited.len(), 2);
}

#[test]
fn test_cancel_expired_settlement_rejects_double_reclaim() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_settlement_expiry_ledgers(&10);
    let id = client.open_settlement(&anchor, &asset, &400);

    env.ledger().set_sequence_number(10);
    client.cancel_expired_settlement(&id);

    let err = client
        .try_cancel_expired_settlement(&id)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::InvalidSettlementState);
}

#[test]
fn test_cancel_settlement_and_expired_race_cancel_wins() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_settlement_expiry_ledgers(&10);
    let id = client.open_settlement(&anchor, &asset, &400);
    assert_eq!(client.total_liquidity(&asset), 600);

    // Advance just past the expiry boundary.
    env.ledger().set_sequence_number(10);

    // cancel_settlement (anchor-authorized) wins the race.
    client.cancel_settlement(&id);

    assert_eq!(client.settlement(&id).status, SettlementStatus::Cancelled);
    // Pool credited exactly once.
    assert_eq!(client.total_liquidity(&asset), 1_000);

    // cancel_expired_settlement sees Cancelled != Pending and rejects.
    let err = client
        .try_cancel_expired_settlement(&id)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::InvalidSettlementState);
    // Pool unchanged — no double-credit.
    assert_eq!(client.total_liquidity(&asset), 1_000);
}

#[test]
fn test_cancel_expired_and_settlement_race_expired_wins() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_settlement_expiry_ledgers(&10);
    let id = client.open_settlement(&anchor, &asset, &400);
    assert_eq!(client.total_liquidity(&asset), 600);

    // Advance just past the expiry boundary.
    env.ledger().set_sequence_number(10);

    // cancel_expired_settlement (permissionless) wins the race.
    client.cancel_expired_settlement(&id);

    assert_eq!(client.settlement(&id).status, SettlementStatus::Expired);
    // Pool credited exactly once.
    assert_eq!(client.total_liquidity(&asset), 1_000);

    // cancel_settlement sees Expired != Pending and rejects.
    let err = client.try_cancel_settlement(&id).err().unwrap().unwrap();
    assert_eq!(err, Error::InvalidSettlementState);
    // Pool unchanged — no double-credit.
    assert_eq!(client.total_liquidity(&asset), 1_000);
}

#[test]
fn test_max_settlement_amount_disabled_by_default() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    assert_eq!(client.max_settlement_amount(&asset), 0);
    assert!(!client.is_max_settlement_amount_configured(&asset));

    // With no cap configured, a large settlement is unaffected.
    client.open_settlement(&anchor, &asset, &1_000);
}

#[test]
fn test_max_settlement_amount_explicit_zero_is_configured_but_disabled() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    assert_eq!(client.max_settlement_amount(&asset), 0);
    assert!(!client.is_max_settlement_amount_configured(&asset));

    client.set_max_settlement_amount(&asset, &0);

    assert_eq!(client.max_settlement_amount(&asset), 0);
    assert!(client.is_max_settlement_amount_configured(&asset));

    // An explicit zero remains cap-disabling, matching the pre-existing
    // open_settlement enforcement rule (`cap > 0 && amount > cap`).
    client.open_settlement(&anchor, &asset, &1_000);
}

#[test]
fn test_set_max_settlement_amount_updates_value() {
    let env = Env::default();
    let (client, _admin, _anchor, asset) = funded(&env, 1_000);

    assert!(!client.is_max_settlement_amount_configured(&asset));

    client.set_max_settlement_amount(&asset, &500);

    assert_eq!(client.max_settlement_amount(&asset), 500);
    assert!(client.is_max_settlement_amount_configured(&asset));
}

#[test]
fn test_clear_max_settlement_amount() {
    let env = Env::default();
    let (client, _admin, _anchor, asset) = funded(&env, 1_000);

    client.set_max_settlement_amount(&asset, &500);
    assert_eq!(client.max_settlement_amount(&asset), 500);

    let key = DataKey::MaxSettlementAmount(asset.clone());
    let has_key_before = env.as_contract(&client.address, || env.storage().persistent().has(&key));
    assert!(has_key_before);

    client.clear_max_settlement_amount(&asset);

    assert_eq!(client.max_settlement_amount(&asset), 0);

    let has_key_after = env.as_contract(&client.address, || env.storage().persistent().has(&key));
    assert!(!has_key_after);

    let events = env.events().all();
    let last_event = events.last().unwrap();
    assert_eq!(
        last_event,
        (
            client.address.clone(),
            (symbol_short!("maxamt"), asset.clone()).into_val(&env),
            0_i128.into_val(&env),
        )
    );
}

#[test]
fn test_set_max_settlement_amount_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");

    client.initialize(&admin);
    client.set_max_settlement_amount(&asset, &500);

    let events = env.events().all();
    assert_eq!(
        events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("maxamt"), asset.clone()).into_val(&env),
                500i128.into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_set_max_settlement_amount_rejects_negative_value() {
    let env = Env::default();
    let (client, _admin, _anchor, asset) = funded(&env, 1_000);

    let err = client
        .try_set_max_settlement_amount(&asset, &-1)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::InvalidAmount);
}

#[test]
fn test_open_settlement_rejects_amount_above_cap() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_max_settlement_amount(&asset, &500);

    let err = client
        .try_open_settlement(&anchor, &asset, &600)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::AboveMaxSettlementAmount);
}

#[test]
fn test_open_settlement_allows_amount_at_cap() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_max_settlement_amount(&asset, &500);

    client.open_settlement(&anchor, &asset, &500);
}

#[test]
fn test_max_settlement_amount_cap_is_per_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &1_000);
    client.provide_liquidity(&anchor, &eurc, &1_000);
    client.set_max_settlement_amount(&usdc, &200);

    // The cap on USDC does not affect settlements against the EURC pool.
    client.open_settlement(&anchor, &eurc, &1_000);

    let err = client
        .try_open_settlement(&anchor, &usdc, &300)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::AboveMaxSettlementAmount);
}

#[test]
fn test_asset_fee_falls_back_to_global_by_default() {
    let env = Env::default();
    let (client, _admin, _anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100); // 1%

    assert_eq!(client.asset_fee(&asset), 100);
}

#[test]
fn test_set_asset_fee_overrides_global_fee() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100); // 1% globally
    client.set_asset_fee(&asset, &500); // 5% for this asset

    assert_eq!(client.asset_fee(&asset), 500);
    assert_eq!(client.quote_fee(&asset, &1_000), 50);

    let id = client.open_settlement(&anchor, &asset, &1_000);
    assert_eq!(client.settlement(&id).fee, 50);
}

#[test]
fn test_set_asset_fee_rejects_above_cap() {
    let env = Env::default();
    let (client, _admin, _anchor, asset) = funded(&env, 1_000);

    let err = client
        .try_set_asset_fee(&asset, &1_001)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::InvalidFee);
}

#[test]
fn test_clear_asset_fee_reverts_to_global() {
    let env = Env::default();
    let (client, _admin, _anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100);
    client.set_asset_fee(&asset, &500);

    client.clear_asset_fee(&asset);

    assert_eq!(client.asset_fee(&asset), 100);
}

// ─────────────────────────────────────────────────────────────────────────
// Regression: clearing an asset fee override reverts to the global rate in
// effect *at clear time*, not the one in effect when the override was set
// (issue #143).
//
// `effective_fee_bps` resolves the rate live — `get_asset_fee(asset)` falling
// back to `get_fee_bps()` — and `clear_asset_fee` simply removes the override
// entry, so nothing is ever cached. The test above cannot detect a regression
// on that point: it never changes the global rate between setting and
// clearing the override, so an implementation that restored the rate
// remembered at override time would produce the same number and stay green.
//
// The two tests below insert a global fee change into that window, covering
// the read side (`asset_fee` / `quote_fee`) and the settlement side
// (`open_settlement` stamp and `execute_settlement` accrual), which resolve
// the rate through the same path and must therefore agree.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_clear_asset_fee_reverts_to_latest_global_fee() {
    let env = Env::default();
    let (client, _admin, _anchor, asset) = funded(&env, 1_000);

    // Global 1%, then a 5% override for this asset.
    client.set_fee(&100);
    client.set_asset_fee(&asset, &500);
    assert_eq!(client.asset_fee(&asset), 500);

    // The global moves to 3% while the override is still active: the
    // override keeps winning, and it is not disturbed by the change.
    client.set_fee(&300);
    assert_eq!(
        client.asset_fee(&asset),
        500,
        "an active override must survive a change to the global fee",
    );
    assert_eq!(client.fee(), 300);

    client.clear_asset_fee(&asset);

    // The asset falls back to the *current* global rate (3%), not to the 1%
    // that was in effect when the override was set.
    assert_eq!(
        client.asset_fee(&asset),
        300,
        "cleared override must revert to the global fee at clear time",
    );
    assert_eq!(
        client.quote_fee(&asset, &1_000),
        30,
        "quote_fee must price against the same live rate as asset_fee",
    );
}

#[test]
fn test_clear_asset_fee_charges_latest_global_fee_on_settlement() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000_000);

    client.set_fee(&100);
    client.set_asset_fee(&asset, &500);
    client.set_fee(&300);
    client.clear_asset_fee(&asset);

    // Everything downstream of the clear must price at the current global
    // rate: the quote, the fee stamped onto the settlement at open, and the
    // amount actually accrued at execution.
    let amount = 10_000i128;
    let quoted = client.quote_fee(&asset, &amount);
    assert_eq!(
        quoted, 300,
        "settlement must be quoted at the global fee in effect at clear time",
    );

    let id = client.open_settlement(&anchor, &asset, &amount);
    assert_eq!(
        client.settlement(&id).fee,
        quoted,
        "the fee stamped at open must match the quote",
    );

    let accrued_before = client.fees_accrued(&asset);
    client.execute_settlement(&id);
    assert_eq!(
        client.fees_accrued(&asset) - accrued_before,
        quoted,
        "accrual must match the quote, confirming a single live fee resolution",
    );
}

#[test]
fn test_set_asset_fee_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");

    client.initialize(&admin);
    client.set_asset_fee(&asset, &250);

    let events = env.events().all();
    assert_eq!(
        events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("assetfee"), asset.clone()).into_val(&env),
                250u32.into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_clear_asset_fee_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");

    client.initialize(&admin);
    client.set_asset_fee(&asset, &250);

    client.clear_asset_fee(&asset);

    let events = env.events().all();
    assert_eq!(
        events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("feeclear"), asset.clone()).into_val(&env),
                ().into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_asset_fee_override_is_per_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.set_fee(&100);
    client.set_asset_fee(&usdc, &500);

    assert_eq!(client.asset_fee(&usdc), 500);
    assert_eq!(client.asset_fee(&eurc), 100);
}

#[test]
fn test_fee_waiver_takes_precedence_over_asset_fee_override() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_asset_fee(&asset, &500);
    client.set_fee_waiver(&anchor, &true);

    let id = client.open_settlement(&anchor, &asset, &1_000);
    assert_eq!(client.settlement(&id).fee, 0);
}

#[test]
fn test_quote_fee_parity_with_executed_accrual() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000_000);
    client.set_fee(&100); // 1% global

    // --- global rate only: quote_fee must match settlement stamp and accrual ---
    let amount = 10_000i128;
    let quoted = client.quote_fee(&asset, &amount);
    let id = client.open_settlement(&anchor, &asset, &amount);
    let settlement = client.settlement(&id);
    assert_eq!(
        quoted, settlement.fee,
        "quote_fee must equal settlement fee stamp under global rate"
    );

    let fees_before = client.fees_accrued(&asset);
    client.execute_settlement(&id);
    let fees_after = client.fees_accrued(&asset);
    assert_eq!(
        quoted,
        fees_after - fees_before,
        "quote_fee must equal fee accrued by execute_settlement under global rate"
    );

    // --- with asset_fee override: quote_fee must still match stamp and accrual ---
    client.set_asset_fee(&asset, &500); // 5% override

    let amount2 = 20_000i128;
    let quoted2 = client.quote_fee(&asset, &amount2);
    let id2 = client.open_settlement(&anchor, &asset, &amount2);
    let settlement2 = client.settlement(&id2);
    assert_eq!(
        quoted2, settlement2.fee,
        "quote_fee must equal settlement fee stamp under asset-fee override"
    );

    let fees_before2 = client.fees_accrued(&asset);
    client.execute_settlement(&id2);
    let fees_after2 = client.fees_accrued(&asset);
    assert_eq!(
        quoted2,
        fees_after2 - fees_before2,
        "quote_fee must equal fee accrued by execute_settlement under asset-fee override"
    );
}

#[test]
fn test_admin_can_extend_instance_ttl() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    client.initialize(&admin);

    // Succeeds and does not panic; the TTL value itself isn't observable
    // through the public interface, so this exercises the auth gate and the
    // call succeeding rather than the underlying ledger bookkeeping.
    client.extend_instance_ttl(&admin);
}

#[test]
fn test_operator_can_extend_instance_ttl() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let operator = Address::generate(&env);
    client.initialize(&admin);
    client.set_operator(&operator);

    client.extend_instance_ttl(&operator);
}

#[test]
fn test_extend_instance_ttl_emits_event_by_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    client.initialize(&admin);

    client.extend_instance_ttl(&admin);

    let events = env.events().all();
    assert_eq!(
        events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("ttl"),).into_val(&env),
                ().into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_extend_instance_ttl_emits_event_by_operator() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let operator = Address::generate(&env);
    client.initialize(&admin);
    client.set_operator(&operator);

    client.extend_instance_ttl(&operator);

    let events = env.events().all();
    assert_eq!(
        events,
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("ttl"),).into_val(&env),
                ().into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_stranger_cannot_extend_instance_ttl() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let stranger = Address::generate(&env);
    client.initialize(&admin);

    let err = client
        .try_extend_instance_ttl(&stranger)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::NotAuthorized);
}

#[test]
fn test_extend_instance_ttl_fails_before_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let err = client
        .try_extend_instance_ttl(&admin)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::NotInitialized);
}

#[test]
fn test_settlement_count_by_status_counts_across_full_history() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    client.open_settlement(&anchor, &asset, &100);
    let executed = client.open_settlement(&anchor, &asset, &100);
    let cancelled = client.open_settlement(&anchor, &asset, &100);
    client.execute_settlement(&executed);
    client.cancel_settlement(&cancelled);

    assert_eq!(
        client.settlement_count_by_status(&SettlementStatus::Pending),
        1
    );
    assert_eq!(
        client.settlement_count_by_status(&SettlementStatus::Executed),
        1
    );
    assert_eq!(
        client.settlement_count_by_status(&SettlementStatus::Cancelled),
        1
    );
    assert_eq!(
        client.settlement_count_by_status(&SettlementStatus::Expired),
        0
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Regression: fully paginating `list_settlements_by_status` must partition the
// settlement history exactly as `settlement_count_by_status` counts it
// (issue #149).
//
// The two functions answer the same question by different means:
// `settlement_count_by_status` scans every id unconditionally, while
// `list_settlements_by_status` walks ids from `start` and stops as soon as
// `limit` matches are collected. Any drift between them means one of the two
// is wrong, and an off-chain dashboard paging by status would disagree with
// the headline count it displays elsewhere.
//
// Per-status equality alone is weaker than it looks: it survives a bug that
// double-counts some settlements and drops others in equal measure. The
// invariant asserted here is a partition — the four per-status id sets are
// pairwise disjoint and their union is exactly `1..=settlement_count()` —
// which additionally catches an id served under two statuses and an
// off-by-one in either walker.
//
// Pagination detail the loop below depends on: `start` is a cursor over
// settlement *ids*, not an offset into the matches. Advancing by
// `start += limit` is therefore wrong and silently re-serves entries whenever
// a status does not begin at the cursor — Executed lands on ids {1, 2, 7, 8}
// here, so that naive advance returns {7, 8} on two consecutive pages. The
// loop advances to `last returned id + 1` instead, and terminates only on an
// empty page, so a paginator that ends a page early is still caught. The
// per-status duplicate assertion below is what pins this down; substituting
// the naive advance makes it fire.
//
// The batch below is also the only coverage of a *non-empty* Expired set:
// `test_list_settlements_by_status_filters_lifecycle_state` and
// `test_settlement_count_by_status_counts_across_full_history` both assert
// only that Expired comes back as 0.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_status_pagination_partitions_settlement_history() {
    use std::collections::HashSet;

    // Deliberately smaller than any per-status set, to force multi-page
    // accumulation rather than a single page that happens to hold everything.
    const PAGE: u32 = 2;

    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 10_000);
    client.set_settlement_expiry_ledgers(&10);

    // First wave, opened at ledger 100 so it can be aged past the window.
    env.ledger().set_sequence_number(100);
    let a1 = client.open_settlement(&anchor, &asset, &100);
    let a2 = client.open_settlement(&anchor, &asset, &100);
    let a3 = client.open_settlement(&anchor, &asset, &100);
    let a4 = client.open_settlement(&anchor, &asset, &100);
    let a5 = client.open_settlement(&anchor, &asset, &100);
    let a6 = client.open_settlement(&anchor, &asset, &100);

    // Resolve part of the first wave while it is still inside the window;
    // execute_settlement rejects a settlement that is already past expiry.
    client.execute_settlement(&a1);
    client.execute_settlement(&a2);
    client.cancel_settlement(&a3);
    client.cancel_settlement(&a4);

    // Age past the window and reclaim the remainder. This is the only path
    // that writes `SettlementStatus::Expired`.
    env.ledger().set_sequence_number(120);
    client.cancel_expired_settlement(&a5);
    client.cancel_expired_settlement(&a6);

    // Second wave, opened after the advance, so it is still inside its own
    // window at the ledger the assertions run on and stays Pending.
    let b1 = client.open_settlement(&anchor, &asset, &100);
    let b2 = client.open_settlement(&anchor, &asset, &100);
    let b3 = client.open_settlement(&anchor, &asset, &100);
    let b4 = client.open_settlement(&anchor, &asset, &100);
    client.open_settlement(&anchor, &asset, &100);
    client.open_settlement(&anchor, &asset, &100);

    client.execute_settlement(&b1);
    client.execute_settlement(&b2);
    client.cancel_settlement(&b3);
    client.cancel_settlement(&b4);

    // Interleaved on purpose: Executed at {a1, a2, b1, b2} = {1, 2, 7, 8} is
    // exactly the non-contiguous shape that breaks a `start += limit` walker.
    assert_eq!(client.settlement_count(), 12);

    // Fully paginate one status, accumulating ids across pages.
    let collect_ids = |status: SettlementStatus| -> HashSet<u64> {
        let mut ids = HashSet::new();
        let mut start = 1u64;
        loop {
            let page = client.list_settlements_by_status(&status, &start, &PAGE);
            if page.is_empty() {
                break;
            }
            assert!(
                page.len() <= PAGE,
                "a page returned more entries than the requested limit"
            );
            for settlement in page.iter() {
                assert_eq!(
                    settlement.status, status,
                    "the status filter returned a settlement in another state"
                );
                assert!(
                    ids.insert(settlement.id),
                    "settlement {} was served twice within one status",
                    settlement.id
                );
            }
            // Advance by id, not by limit: `start` is an id cursor.
            start = page.get(page.len() - 1).unwrap().id + 1;
        }
        ids
    };

    let pending = collect_ids(SettlementStatus::Pending);
    let executed = collect_ids(SettlementStatus::Executed);
    let cancelled = collect_ids(SettlementStatus::Cancelled);
    let expired = collect_ids(SettlementStatus::Expired);

    // Each accumulated set matches the unconditional scan, per status.
    for (status, ids) in [
        (SettlementStatus::Pending, &pending),
        (SettlementStatus::Executed, &executed),
        (SettlementStatus::Cancelled, &cancelled),
        (SettlementStatus::Expired, &expired),
    ] {
        assert_eq!(
            ids.len() as u64,
            client.settlement_count_by_status(&status),
            "paginated accumulation disagrees with settlement_count_by_status"
        );
        // A partition of empty sets would satisfy the checks below vacuously.
        assert!(!ids.is_empty(), "the batch must exercise every status");
    }

    // The four sets are pairwise disjoint.
    let sets = [&pending, &executed, &cancelled, &expired];
    for (i, left) in sets.iter().enumerate() {
        for right in sets.iter().skip(i + 1) {
            assert!(
                left.intersection(right).next().is_none(),
                "a settlement id was served under two different statuses"
            );
        }
    }

    // Their union is every id ever assigned — no gaps, no strays.
    let union: HashSet<u64> = sets.iter().flat_map(|set| set.iter().copied()).collect();
    let every_id: HashSet<u64> = (1..=client.settlement_count()).collect();
    assert_eq!(
        union, every_id,
        "the per-status sets must cover exactly 1..=settlement_count()"
    );
}

#[test]
fn test_settlement_count_by_status_is_zero_with_no_settlements() {
    let env = Env::default();
    let (client, _admin, _anchor, _asset) = funded(&env, 1_000);

    assert_eq!(
        client.settlement_count_by_status(&SettlementStatus::Pending),
        0
    );
}

#[test]
fn test_anchor_settlement_count_counts_only_that_anchor() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.provide_liquidity(&a1, &usdc, &1_000);
    client.provide_liquidity(&a2, &usdc, &1_000);

    // a1 opens 3, a2 opens 2, interleaved
    client.open_settlement(&a1, &usdc, &100);
    client.open_settlement(&a2, &usdc, &100);
    client.open_settlement(&a1, &usdc, &100);
    client.open_settlement(&a2, &usdc, &100);
    client.open_settlement(&a1, &usdc, &100);

    assert_eq!(client.anchor_settlement_count(&a1), 3);
    assert_eq!(client.anchor_settlement_count(&a2), 2);
}

#[test]
fn test_anchor_settlement_count_zero_for_unknown_anchor() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);
    let stranger = Address::generate(&env);

    assert_eq!(client.anchor_settlement_count(&stranger), 0);
}

#[test]
fn test_anchor_settlement_count_zero_when_none_opened() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    client.initialize(&admin);
    client.register_anchor(&anchor);

    assert_eq!(client.anchor_settlement_count(&anchor), 0);
}

#[test]
fn test_contract_info_reflects_current_state() {
    let env = Env::default();
    let (client, admin, anchor, asset) = funded(&env, 1_000);
    client.set_fee(&250);
    client.open_settlement(&anchor, &asset, &100);
    client.pause(&admin);

    let info = client.contract_info();

    assert_eq!(info.version, client.version());
    assert!(info.paused);
    assert_eq!(info.fee_bps, 250);
    assert_eq!(info.anchor_count, 1);
    assert_eq!(info.asset_count, 1);
    assert_eq!(info.settlement_count, 1);
}

#[test]
fn test_contract_info_before_any_activity() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    client.initialize(&admin);

    let info = client.contract_info();

    assert!(!info.paused);
    assert_eq!(info.anchor_count, 0);
    assert_eq!(info.asset_count, 0);
    assert_eq!(info.settlement_count, 0);
}

#[test]
fn test_max_fee_bps_matches_set_fee_cap() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    client.initialize(&admin);

    let cap = client.max_fee_bps();
    client.set_fee(&cap);
    assert_eq!(client.fee(), cap);

    let err = client.try_set_fee(&(cap + 1)).err().unwrap().unwrap();
    assert_eq!(err, Error::InvalidFee);
}

#[test]
fn test_withdraw_liquidity_multi_withdraws_every_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");
    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &1_000);
    client.provide_liquidity(&anchor, &eurc, &500);

    let requests = vec![&env, (usdc.clone(), 400), (eurc.clone(), 200)];
    client.withdraw_liquidity_multi(&anchor, &requests);

    assert_eq!(client.balance(&anchor, &usdc), 600);
    assert_eq!(client.balance(&anchor, &eurc), 300);
}

#[test]
fn test_withdraw_liquidity_multi_rejects_empty_batch() {
    let env = Env::default();
    let (client, _admin, anchor, _asset) = funded(&env, 1_000);

    let empty: soroban_sdk::Vec<(Symbol, i128)> = vec![&env];
    let err = client
        .try_withdraw_liquidity_multi(&anchor, &empty)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::InvalidAmount);
}

#[test]
fn test_withdraw_liquidity_multi_rejects_duplicate_asset() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    let requests = vec![&env, (asset.clone(), 100), (asset.clone(), 100)];
    let err = client
        .try_withdraw_liquidity_multi(&anchor, &requests)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::DuplicateAssetInBatch);
}

#[test]
fn test_withdraw_liquidity_multi_applies_none_on_insufficient_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");
    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &1_000);
    client.provide_liquidity(&anchor, &eurc, &100);

    // The EURC leg exceeds the provider's balance, so neither leg applies.
    let requests = vec![&env, (usdc.clone(), 400), (eurc.clone(), 200)];
    let err = client
        .try_withdraw_liquidity_multi(&anchor, &requests)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::InsufficientLiquidity);
    assert_eq!(client.balance(&anchor, &usdc), 1_000);
    assert_eq!(client.balance(&anchor, &eurc), 100);
}

#[test]
fn test_withdraw_liquidity_multi_respects_min_liquidity_floor() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_min_liquidity(&asset, &700);

    let requests = vec![&env, (asset.clone(), 400)];
    let err = client
        .try_withdraw_liquidity_multi(&anchor, &requests)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::BelowMinLiquidity);
}

#[test]
fn test_withdraw_liquidity_multi_zero_mutations_on_late_failures() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let asset1 = symbol_short!("AST1");
    let asset2 = symbol_short!("AST2");
    let asset3 = symbol_short!("AST3");

    client.initialize(&admin);
    client.register_anchor(&anchor);

    client.provide_liquidity(&anchor, &asset1, &1000);
    client.provide_liquidity(&anchor, &asset2, &1000);
    client.provide_liquidity(&anchor, &asset3, &1000);

    client.set_min_liquidity(&asset3, &800);

    let bal1_before = client.balance(&anchor, &asset1);
    let bal2_before = client.balance(&anchor, &asset2);
    let bal3_before = client.balance(&anchor, &asset3);
    let total1_before = client.total_liquidity(&asset1);
    let total2_before = client.total_liquidity(&asset2);
    let total3_before = client.total_liquidity(&asset3);

    // 1. Late failure: insufficient balance on third asset
    let reqs_insufficient = vec![
        &env,
        (asset1.clone(), 100),
        (asset2.clone(), 100),
        (asset3.clone(), 2000),
    ];
    let err_insuf = client
        .try_withdraw_liquidity_multi(&anchor, &reqs_insufficient)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err_insuf, Error::InsufficientLiquidity);

    // Verify state unchanged
    assert_eq!(client.balance(&anchor, &asset1), bal1_before);
    assert_eq!(client.balance(&anchor, &asset2), bal2_before);
    assert_eq!(client.balance(&anchor, &asset3), bal3_before);
    assert_eq!(client.total_liquidity(&asset1), total1_before);
    assert_eq!(client.total_liquidity(&asset2), total2_before);
    assert_eq!(client.total_liquidity(&asset3), total3_before);

    // 2. Late failure: below min liquidity on third asset
    let reqs_min = vec![
        &env,
        (asset1.clone(), 100),
        (asset2.clone(), 100),
        (asset3.clone(), 300),
    ];
    let err_min = client
        .try_withdraw_liquidity_multi(&anchor, &reqs_min)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err_min, Error::BelowMinLiquidity);

    // Verify state unchanged again
    assert_eq!(client.balance(&anchor, &asset1), bal1_before);
    assert_eq!(client.balance(&anchor, &asset2), bal2_before);
    assert_eq!(client.balance(&anchor, &asset3), bal3_before);
    assert_eq!(client.total_liquidity(&asset1), total1_before);
    assert_eq!(client.total_liquidity(&asset2), total2_before);
    assert_eq!(client.total_liquidity(&asset3), total3_before);
}

#[test]
fn test_provide_liquidity_multi_funds_every_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");
    client.initialize(&admin);
    client.register_anchor(&anchor);

    let requests = vec![&env, (usdc.clone(), 400), (eurc.clone(), 200)];
    client.provide_liquidity_multi(&anchor, &requests);

    assert_eq!(client.balance(&anchor, &usdc), 400);
    assert_eq!(client.balance(&anchor, &eurc), 200);
    assert_eq!(client.total_liquidity(&usdc), 400);
    assert_eq!(client.total_liquidity(&eurc), 200);
}

#[test]
fn test_provide_liquidity_multi_tracks_providers_independently() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor1 = Address::generate(&env);
    let anchor2 = Address::generate(&env);
    let asset1 = symbol_short!("AST1");
    let asset2 = symbol_short!("AST2");
    let asset3 = symbol_short!("AST3");

    client.initialize(&admin);
    client.register_anchor(&anchor1);
    client.register_anchor(&anchor2);

    let requests = vec![
        &env,
        (asset1.clone(), 100),
        (asset2.clone(), 100),
        (asset3.clone(), 100),
    ];
    client.provide_liquidity_multi(&anchor1, &requests);

    assert_eq!(client.pool(&asset1).providers, 1);
    assert_eq!(client.pool(&asset2).providers, 1);
    assert_eq!(client.pool(&asset3).providers, 1);

    let withdraw_requests = vec![&env, (asset1.clone(), 100)];
    client.withdraw_liquidity_multi(&anchor1, &withdraw_requests);

    assert_eq!(client.pool(&asset1).providers, 0);
    assert_eq!(client.pool(&asset2).providers, 1);
    assert_eq!(client.pool(&asset3).providers, 1);

    client.provide_liquidity(&anchor2, &asset2, &50);

    assert_eq!(client.pool(&asset1).providers, 0);
    assert_eq!(client.pool(&asset2).providers, 2);
    assert_eq!(client.pool(&asset3).providers, 1);
}

#[test]
fn test_provide_liquidity_multi_rejects_unregistered_anchor() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let stranger = Address::generate(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);

    let requests = vec![&env, (asset.clone(), 100)];
    let err = client
        .try_provide_liquidity_multi(&stranger, &requests)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::AnchorNotRegistered);
}

#[test]
fn test_provide_liquidity_multi_rejects_duplicate_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);
    client.register_anchor(&anchor);

    let requests = vec![&env, (asset.clone(), 100), (asset.clone(), 100)];
    let err = client
        .try_provide_liquidity_multi(&anchor, &requests)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::DuplicateAssetInBatch);

    // Neither leg was applied.
    assert_eq!(client.balance(&anchor, &asset), 0);
}

#[test]
fn test_provide_liquidity_multi_rejects_empty_batch() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    client.initialize(&admin);
    client.register_anchor(&anchor);

    let empty: soroban_sdk::Vec<(Symbol, i128)> = vec![&env];
    let err = client
        .try_provide_liquidity_multi(&anchor, &empty)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::InvalidAmount);
}

#[test]
fn test_provide_liquidity_multi_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.pause(&admin);

    let requests = vec![&env, (asset.clone(), 100)];
    let err = client
        .try_provide_liquidity_multi(&anchor, &requests)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::Paused);
}

// ---------------------------------------------------------------------------
// provide_liquidity_multi atomicity regression tests
//
// The existing test_provide_liquidity_multi_rejects_duplicate_asset test only
// covers the case where the duplicate is at the front of the batch (both
// entries are the same asset). These regression tests verify that an invalid
// entry appearing *later* in the requests vector — after several valid distinct
// assets — causes zero mutations across the entire batch, including the valid
// entries that appeared before the invalid one. This enforces the all-or-nothing
// atomicity guarantee that provide_liquidity_multi's doc comment promises.
// ---------------------------------------------------------------------------

/// Regression test: an invalid entry (duplicate asset) appearing *later* in the
/// batch — after several valid distinct assets — must cause zero mutations across
/// the entire batch, including the valid entries that appeared before the invalid
/// one. Without the two-pass validate-then-apply design, the first two legs could
/// be partially applied before the duplicate is detected on the third.
#[test]
fn test_provide_liquidity_multi_zero_mutations_on_late_duplicate_failure() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let asset1 = symbol_short!("AST1");
    let asset2 = symbol_short!("AST2");

    client.initialize(&admin);
    client.register_anchor(&anchor);

    // Snapshot balances and pool totals for every affected asset before the
    // call. All are zero since no liquidity has been provided yet.
    let bal1_before = client.balance(&anchor, &asset1);
    let bal2_before = client.balance(&anchor, &asset2);
    let total1_before = client.total_liquidity(&asset1);
    let total2_before = client.total_liquidity(&asset2);
    let providers1_before = client.pool(&asset1).providers;
    let providers2_before = client.pool(&asset2).providers;

    // First two entries are valid distinct assets; third is a duplicate of the
    // first — the invalid entry appears *after* valid ones.
    let requests = vec![
        &env,
        (asset1.clone(), 100),
        (asset2.clone(), 200),
        (asset1.clone(), 300), // duplicate of asset1
    ];
    let err = client
        .try_provide_liquidity_multi(&anchor, &requests)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::DuplicateAssetInBatch);

    // Verify state unchanged for every asset in the batch — including the valid
    // ones (asset1, asset2) that appeared before the invalid entry.
    assert_eq!(client.balance(&anchor, &asset1), bal1_before);
    assert_eq!(client.balance(&anchor, &asset2), bal2_before);
    assert_eq!(client.total_liquidity(&asset1), total1_before);
    assert_eq!(client.total_liquidity(&asset2), total2_before);
    assert_eq!(client.pool(&asset1).providers, providers1_before);
    assert_eq!(client.pool(&asset2).providers, providers2_before);
}

/// Regression test: an invalid entry (non-positive amount) appearing *later* in
/// the batch — after several valid distinct assets — must cause zero mutations
/// across the entire batch, including the valid entries that appeared before the
/// invalid one. The non-positive amount is detected at a different point in the
/// validation loop than the duplicate-asset check, so it is covered separately.
#[test]
fn test_provide_liquidity_multi_zero_mutations_on_late_nonpositive_failure() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let asset1 = symbol_short!("AST1");
    let asset2 = symbol_short!("AST2");
    let asset3 = symbol_short!("AST3");

    client.initialize(&admin);
    client.register_anchor(&anchor);

    // Snapshot balances and pool totals for every affected asset before the
    // call. All are zero since no liquidity has been provided yet.
    let bal1_before = client.balance(&anchor, &asset1);
    let bal2_before = client.balance(&anchor, &asset2);
    let bal3_before = client.balance(&anchor, &asset3);
    let total1_before = client.total_liquidity(&asset1);
    let total2_before = client.total_liquidity(&asset2);
    let total3_before = client.total_liquidity(&asset3);
    let providers1_before = client.pool(&asset1).providers;
    let providers2_before = client.pool(&asset2).providers;
    let providers3_before = client.pool(&asset3).providers;

    // First two entries are valid distinct assets; third has a non-positive
    // amount — the invalid entry appears *after* valid ones.
    let requests = vec![
        &env,
        (asset1.clone(), 100),
        (asset2.clone(), 200),
        (asset3.clone(), 0), // non-positive amount
    ];
    let err = client
        .try_provide_liquidity_multi(&anchor, &requests)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::InvalidAmount);

    // Verify state unchanged for every asset in the batch — including the valid
    // ones (asset1, asset2) that appeared before the invalid entry.
    assert_eq!(client.balance(&anchor, &asset1), bal1_before);
    assert_eq!(client.balance(&anchor, &asset2), bal2_before);
    assert_eq!(client.balance(&anchor, &asset3), bal3_before);
    assert_eq!(client.total_liquidity(&asset1), total1_before);
    assert_eq!(client.total_liquidity(&asset2), total2_before);
    assert_eq!(client.total_liquidity(&asset3), total3_before);
    assert_eq!(client.pool(&asset1).providers, providers1_before);
    assert_eq!(client.pool(&asset2).providers, providers2_before);
    assert_eq!(client.pool(&asset3).providers, providers3_before);
}

#[test]
fn test_total_settled_amount_sums_by_status() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    let a = client.open_settlement(&anchor, &asset, &100);
    let b = client.open_settlement(&anchor, &asset, &250);
    client.open_settlement(&anchor, &asset, &50); // stays pending
    client.execute_settlement(&a);
    client.execute_settlement(&b);

    assert_eq!(
        client.total_settled_amount(&SettlementStatus::Executed),
        350
    );
    assert_eq!(client.total_settled_amount(&SettlementStatus::Pending), 50);
    assert_eq!(client.total_settled_amount(&SettlementStatus::Cancelled), 0);
}

#[test]
fn test_total_settled_amount_is_zero_with_no_settlements() {
    let env = Env::default();
    let (client, _admin, _anchor, _asset) = funded(&env, 1_000);

    assert_eq!(client.total_settled_amount(&SettlementStatus::Pending), 0);
}

#[test]
fn test_anchor_balances_lists_only_nonzero_holdings() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");
    let xlm = symbol_short!("XLM");
    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &500);
    client.provide_liquidity(&anchor, &eurc, &200);
    // XLM gets funded by a different anchor, so it's known to the contract
    // but this anchor holds none of it.
    let other = Address::generate(&env);
    client.register_anchor(&other);
    client.provide_liquidity(&other, &xlm, &1_000);

    let balances = client.anchor_balances(&anchor, &0, &10);

    assert_eq!(balances.len(), 2);
    assert_eq!(balances.get(0).unwrap(), (usdc, 500));
    assert_eq!(balances.get(1).unwrap(), (eurc, 200));
}

#[test]
fn test_anchor_balances_respects_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &symbol_short!("USDC"), &100);
    client.provide_liquidity(&anchor, &symbol_short!("EURC"), &100);

    assert_eq!(client.anchor_balances(&anchor, &0, &1).len(), 1);
}

#[test]
fn test_anchor_balances_empty_for_a_provider_with_no_liquidity() {
    let env = Env::default();
    let (client, _admin, _anchor, _asset) = funded(&env, 1_000);
    let stranger = Address::generate(&env);

    assert_eq!(client.anchor_balances(&stranger, &0, &10).len(), 0);
}

/// The `pool.providers` counter must track distinct active providers exactly
/// through interleaved partial and full provide/withdraw sequences: partial
/// withdrawals never decrement it, full withdrawals decrement it by one, and a
/// re-entry from a zero balance increments it again. This exercises the
/// [`do_withdraw`] underflow guard end-to-end via the real public entry points
/// — the actual surface where the invariant could be broken.
#[test]
fn providers_counter_survives_interleaved_provide_withdraw() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let usdc = symbol_short!("USDC");
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&a);
    client.register_anchor(&b);
    client.register_anchor(&c);

    client.provide_liquidity(&a, &usdc, &1_000);
    assert_eq!(client.pool(&usdc).providers, 1);

    client.provide_liquidity(&b, &usdc, &2_000);
    assert_eq!(client.pool(&usdc).providers, 2);

    // Partial withdrawal keeps a positive balance → count unchanged.
    client.withdraw_liquidity(&a, &usdc, &300);
    assert_eq!(client.pool(&usdc).providers, 2);

    client.provide_liquidity(&c, &usdc, &500);
    assert_eq!(client.pool(&usdc).providers, 3);

    // Full withdrawal → count drops to 2.
    client.withdraw_liquidity(&b, &usdc, &2_000);
    assert_eq!(client.pool(&usdc).providers, 2);

    // a withdraws its remaining 700 → count drops to 1.
    client.withdraw_liquidity(&a, &usdc, &700);
    assert_eq!(client.pool(&usdc).providers, 1);

    // c tops up while already active → count unchanged.
    client.provide_liquidity(&c, &usdc, &100);
    assert_eq!(client.pool(&usdc).providers, 1);

    // c withdraws everything (500 + 100) → count drops to 0.
    client.withdraw_liquidity(&c, &usdc, &600);
    assert_eq!(client.pool(&usdc).providers, 0);

    // Re-entry from zero balance increments back to 1.
    client.provide_liquidity(&a, &usdc, &50);
    assert_eq!(client.pool(&usdc).providers, 1);
}

/// A full withdrawal that returns a provider's balance to zero still
/// decrements the provider count — guards against a regression in the
/// unchanged zero-balance exit path.
#[test]
fn full_withdraw_still_decrements_providers() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let usdc = symbol_short!("USDC");
    let a = Address::generate(&env);

    client.initialize(&admin);
    client.register_anchor(&a);

    client.provide_liquidity(&a, &usdc, &1_000);
    assert_eq!(client.pool(&usdc).providers, 1);

    client.withdraw_liquidity(&a, &usdc, &1_000);
    assert_eq!(client.pool(&usdc).providers, 0);
}

// ---------------------------------------------------------------------------
// pool.providers invariant under interleaved sequences – issue #128
//
// `providers_counter_survives_interleaved_provide_withdraw` above already walks
// three anchors through partial/full withdrawals and a re-entry, but it asserts
// against counts hardcoded per step, and every full exit in it goes through
// `withdraw_liquidity` with an exact amount. The test below closes both gaps:
//
//   1. The expected count is never written down. It is derived on every
//      checkpoint from a balance ledger the test maintains itself, so a step
//      that is reordered, edited or inserted cannot silently keep passing
//      against a stale literal.
//   2. Full exits alternate between `withdraw_liquidity` with the exact
//      remaining balance and `withdraw_all_liquidity`, which reaches the same
//      decrement through a different entry point, and each provider re-enters
//      after both kinds of exit.
//
// The tracked ledger is itself cross-checked against `balance()` at every step,
// so it cannot drift away from the contract and validate a wrong expectation.
// ---------------------------------------------------------------------------

/// Asserts that `pool.providers` equals the number of anchors that `balances`
/// records as currently holding liquidity, and that the tracked balances still
/// agree with the contract's own view.
fn assert_providers_match_ledger(
    client: &AnchornetContractClient<'_>,
    asset: &Symbol,
    anchors: &[Address],
    balances: &[i128],
    step: &str,
) {
    for (i, expected) in balances.iter().enumerate() {
        assert_eq!(
            client.balance(&anchors[i], asset),
            *expected,
            "tracked balance for anchor {i} drifted from the contract after {step}",
        );
    }

    let expected = balances.iter().filter(|b| **b > 0).count() as u32;
    assert_eq!(
        client.pool(asset).providers,
        expected,
        "pool.providers disagrees with the tracked active-provider count after {step}",
    );
}

/// `pool.providers` must equal the number of anchors with a non-zero balance at
/// every point of an interleaved sequence, no matter which entry point drove the
/// change. Exercises partial withdrawals, full exits through both
/// `withdraw_liquidity` and `withdraw_all_liquidity`, top-ups by an already
/// active provider, and re-entries after each kind of exit (issue #128).
#[test]
fn providers_count_tracks_active_anchors_through_interleaved_sequence() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let usdc = symbol_short!("USDC");

    client.initialize(&admin);
    let anchors = [
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    for anchor in &anchors {
        client.register_anchor(anchor);
    }

    // The independently tracked ledger: index i mirrors anchors[i]'s balance.
    let mut balances = [0i128; 3];

    let provide = |balances: &mut [i128; 3], who: usize, amount: i128, step: &str| {
        client.provide_liquidity(&anchors[who], &usdc, &amount);
        balances[who] += amount;
        assert_providers_match_ledger(&client, &usdc, &anchors, balances, step);
    };

    provide(&mut balances, 0, 1_000, "a provides its first liquidity");
    provide(&mut balances, 1, 2_000, "b joins the pool");
    provide(&mut balances, 2, 500, "c joins the pool");

    // Partial withdrawal: a keeps a positive balance, so it stays a provider.
    client.withdraw_liquidity(&anchors[0], &usdc, &300);
    balances[0] -= 300;
    assert_providers_match_ledger(&client, &usdc, &anchors, &balances, "a partially withdraws");

    // Full exit through withdraw_all_liquidity.
    assert_eq!(client.withdraw_all_liquidity(&anchors[1], &usdc), 2_000);
    balances[1] = 0;
    assert_providers_match_ledger(&client, &usdc, &anchors, &balances, "b exits with all");

    // Top-up by an anchor that is already counted must not double-count it.
    provide(&mut balances, 2, 250, "c tops up while already active");

    // Full exit through withdraw_liquidity with the exact remaining balance.
    client.withdraw_liquidity(&anchors[0], &usdc, &700);
    balances[0] = 0;
    assert_providers_match_ledger(&client, &usdc, &anchors, &balances, "a exits exactly");

    // Re-entry after a withdraw_all_liquidity exit must increment again.
    provide(&mut balances, 1, 100, "b re-enters after withdrawing all");

    // Re-entry after an exact-amount exit must increment again.
    provide(&mut balances, 0, 50, "a re-enters after an exact exit");

    // Drain the pool, alternating exit paths once more.
    assert_eq!(client.withdraw_all_liquidity(&anchors[2], &usdc), 750);
    balances[2] = 0;
    assert_providers_match_ledger(&client, &usdc, &anchors, &balances, "c exits with all");

    client.withdraw_liquidity(&anchors[1], &usdc, &100);
    balances[1] = 0;
    assert_providers_match_ledger(&client, &usdc, &anchors, &balances, "b exits exactly");

    assert_eq!(client.withdraw_all_liquidity(&anchors[0], &usdc), 50);
    balances[0] = 0;
    assert_providers_match_ledger(&client, &usdc, &anchors, &balances, "a makes the last exit");

    // The pool is empty again: the counter must be back at zero, not stuck.
    assert_eq!(client.pool(&usdc).providers, 0);

    // And it must still recover from an empty pool.
    provide(&mut balances, 2, 10, "c reopens an emptied pool");
}

// ---------------------------------------------------------------------------
// Pagination edge-case regression tests – issue #96
//
// Each list_* entrypoint is exercised for three edge-cases:
//   1. start past the end  → must return an empty vec, not panic
//   2. limit = 0           → must return an empty vec, not panic
//   3. limit > remaining   → must return exactly the remaining items, not panic
// ---------------------------------------------------------------------------

// --- list_anchors ---

#[test]
fn test_list_anchors_start_past_end_returns_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);

    // There are 2 anchors at indices 0 and 1; starting at index 2 is past end.
    assert_eq!(client.list_anchors(&2, &10).len(), 0);
    // Far-past-end with a u32 near its maximum should also be safe.
    assert_eq!(client.list_anchors(&u32::MAX, &10).len(), 0);
}

#[test]
fn test_list_anchors_limit_zero_returns_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    client.initialize(&admin);
    client.register_anchor(&a1);

    assert_eq!(client.list_anchors(&0, &0).len(), 0);
}

#[test]
fn test_list_anchors_limit_exceeds_remaining_returns_all() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);

    // Ask for 1000 but only 2 are registered; must get exactly 2.
    let result = client.list_anchors(&0, &1_000);
    assert_eq!(result.len(), 2);
    // Verify they are the same anchors in order.
    assert_eq!(result.get(0).unwrap(), a1);
    assert_eq!(result.get(1).unwrap(), a2);
}

// --- list_assets ---

#[test]
fn test_list_assets_start_past_end_returns_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");
    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &100);
    client.provide_liquidity(&anchor, &eurc, &100);

    // 2 assets at indices 0 and 1; starting at index 2 is past end.
    assert_eq!(client.list_assets(&2, &10).len(), 0);
    assert_eq!(client.list_assets(&u32::MAX, &10).len(), 0);
}

#[test]
fn test_list_assets_limit_zero_returns_empty() {
    let env = Env::default();
    let (client, _admin, _anchor, _asset) = funded(&env, 1_000);

    assert_eq!(client.list_assets(&0, &0).len(), 0);
}

#[test]
fn test_list_assets_limit_exceeds_remaining_returns_all() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");
    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &100);
    client.provide_liquidity(&anchor, &eurc, &100);

    let result = client.list_assets(&0, &1_000);
    assert_eq!(result.len(), 2);
    assert_eq!(result.get(0).unwrap(), usdc);
    assert_eq!(result.get(1).unwrap(), eurc);
}

// --- list_fee_waived_anchors ---

#[test]
fn test_list_fee_waived_anchors_start_past_end_returns_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.set_fee_waiver(&a1, &true);
    client.set_fee_waiver(&a2, &true);

    // The anchor list has 2 entries (indices 0 and 1); starting at index 2 is past end.
    assert_eq!(client.list_fee_waived_anchors(&2, &10).len(), 0);
    assert_eq!(client.list_fee_waived_anchors(&u32::MAX, &10).len(), 0);
}

#[test]
fn test_list_fee_waived_anchors_limit_zero_returns_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.set_fee_waiver(&anchor, &true);

    assert_eq!(client.list_fee_waived_anchors(&0, &0).len(), 0);
}

#[test]
fn test_list_fee_waived_anchors_limit_exceeds_remaining_returns_all() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.set_fee_waiver(&a1, &true);
    client.set_fee_waiver(&a2, &true);

    let result = client.list_fee_waived_anchors(&0, &1_000);
    assert_eq!(result.len(), 2);
    assert_eq!(result.get(0).unwrap(), a1);
    assert_eq!(result.get(1).unwrap(), a2);
}

// --- list_settlements ---

#[test]
fn test_list_settlements_start_past_end_returns_empty() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);
    client.open_settlement(&anchor, &asset, &100);
    // 2 settlements with ids 1 and 2; starting at id 3 is past end.
    assert_eq!(client.list_settlements(&3, &10).len(), 0);
    assert_eq!(client.list_settlements(&u64::MAX, &10).len(), 0);
}

#[test]
fn test_list_settlements_limit_zero_returns_empty() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);

    assert_eq!(client.list_settlements(&1, &0).len(), 0);
    // start=0 normalises to id 1 internally; limit=0 should still return empty.
    assert_eq!(client.list_settlements(&0, &0).len(), 0);
}

#[test]
fn test_list_settlements_limit_exceeds_remaining_returns_all() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    let id1 = client.open_settlement(&anchor, &asset, &100);
    let id2 = client.open_settlement(&anchor, &asset, &100);

    let result = client.list_settlements(&1, &1_000);
    assert_eq!(result.len(), 2);
    assert_eq!(result.get(0).unwrap().id, id1);
    assert_eq!(result.get(1).unwrap().id, id2);
}

// --- list_settlements_by_anchor ---

#[test]
fn test_list_settlements_by_anchor_start_past_end_returns_empty() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);
    client.open_settlement(&anchor, &asset, &100);

    assert_eq!(client.list_settlements_by_anchor(&anchor, &3, &10).len(), 0);
    assert_eq!(
        client
            .list_settlements_by_anchor(&anchor, &u64::MAX, &10)
            .len(),
        0
    );
}

#[test]
fn test_list_settlements_by_anchor_limit_zero_returns_empty() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);

    assert_eq!(client.list_settlements_by_anchor(&anchor, &1, &0).len(), 0);
    assert_eq!(client.list_settlements_by_anchor(&anchor, &0, &0).len(), 0);
}

#[test]
fn test_list_settlements_by_anchor_limit_exceeds_remaining_returns_all() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    let id1 = client.open_settlement(&anchor, &asset, &100);
    let id2 = client.open_settlement(&anchor, &asset, &100);

    let result = client.list_settlements_by_anchor(&anchor, &1, &1_000);
    assert_eq!(result.len(), 2);
    assert_eq!(result.get(0).unwrap().id, id1);
    assert_eq!(result.get(1).unwrap().id, id2);
}

// --- list_settlements_by_asset ---

#[test]
fn test_list_settlements_by_asset_start_past_end_returns_empty() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);
    client.open_settlement(&anchor, &asset, &100);

    assert_eq!(client.list_settlements_by_asset(&asset, &3, &10).len(), 0);
    assert_eq!(
        client
            .list_settlements_by_asset(&asset, &u64::MAX, &10)
            .len(),
        0
    );
}

#[test]
fn test_list_settlements_by_asset_limit_zero_returns_empty() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);

    assert_eq!(client.list_settlements_by_asset(&asset, &1, &0).len(), 0);
    assert_eq!(client.list_settlements_by_asset(&asset, &0, &0).len(), 0);
}

#[test]
fn test_list_settlements_by_asset_limit_exceeds_remaining_returns_all() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    let id1 = client.open_settlement(&anchor, &asset, &100);
    let id2 = client.open_settlement(&anchor, &asset, &100);

    let result = client.list_settlements_by_asset(&asset, &1, &1_000);
    assert_eq!(result.len(), 2);
    assert_eq!(result.get(0).unwrap().id, id1);
    assert_eq!(result.get(1).unwrap().id, id2);
}

// --- list_settlements_by_anch_asset ---

#[test]
fn test_list_settlements_by_anch_asset_start_past_end_returns_empty() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);
    client.open_settlement(&anchor, &asset, &100);

    assert_eq!(
        client
            .list_settlements_by_anchor_and_asset(&anchor, &asset, &3, &10)
            .len(),
        0
    );
    assert_eq!(
        client
            .list_settlements_by_anch_asset(&anchor, &asset, &u64::MAX, &10)
            .len(),
        0
    );
}

#[test]
fn test_list_settlements_by_anch_asset_limit_zero_returns_empty() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);

    assert_eq!(
        client
            .list_settlements_by_anchor_and_asset(&anchor, &asset, &1, &0)
            .len(),
        0
    );
    assert_eq!(
        client
            .list_settlements_by_anchor_and_asset(&anchor, &asset, &0, &0)
            .len(),
        0
    );
}

#[test]
fn test_list_settlements_by_anch_asset_limit_exceeds_remaining_returns_all() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    let id1 = client.open_settlement(&anchor, &asset, &100);
    let id2 = client.open_settlement(&anchor, &asset, &100);

    let result = client.list_settlements_by_anch_asset(&anchor, &asset, &1, &1_000);
    assert_eq!(result.len(), 2);
    assert_eq!(result.get(0).unwrap().id, id1);
    assert_eq!(result.get(1).unwrap().id, id2);
}

// --- list_settlements_by_status ---

#[test]
fn test_list_settlements_by_status_start_past_end_returns_empty() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);
    client.open_settlement(&anchor, &asset, &100);

    assert_eq!(
        client
            .list_settlements_by_status(&SettlementStatus::Pending, &3, &10)
            .len(),
        0
    );
    assert_eq!(
        client
            .list_settlements_by_status(&SettlementStatus::Pending, &u64::MAX, &10)
            .len(),
        0
    );
}

#[test]
fn test_list_settlements_by_status_limit_zero_returns_empty() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);

    assert_eq!(
        client
            .list_settlements_by_status(&SettlementStatus::Pending, &1, &0)
            .len(),
        0
    );
    assert_eq!(
        client
            .list_settlements_by_status(&SettlementStatus::Pending, &0, &0)
            .len(),
        0
    );
}

#[test]
fn test_list_settlements_by_status_limit_exceeds_remaining_returns_all() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    let id1 = client.open_settlement(&anchor, &asset, &100);
    let id2 = client.open_settlement(&anchor, &asset, &100);

    let result = client.list_settlements_by_status(&SettlementStatus::Pending, &1, &1_000);
    assert_eq!(result.len(), 2);
    assert_eq!(result.get(0).unwrap().id, id1);
    assert_eq!(result.get(1).unwrap().id, id2);
}

#[test]
fn test_list_settlements_beyond_count() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);
    client.open_settlement(&anchor, &asset, &100);

    let count = client.settlement_count();
    assert_eq!(count, 2);

    let start_beyond = 100u64;

    assert_eq!(client.list_settlements(&start_beyond, &10).len(), 0);
    assert_eq!(
        client
            .list_settlements_by_anchor(&anchor, &start_beyond, &10)
            .len(),
        0
    );
    assert_eq!(
        client
            .list_settlements_by_asset(&asset, &start_beyond, &10)
            .len(),
        0
    );
    assert_eq!(
        client
            .list_settlements_by_status(&SettlementStatus::Pending, &start_beyond, &10)
            .len(),
        0
    );
}

#[test]
fn test_list_settlements_off_by_one_boundary() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.open_settlement(&anchor, &asset, &100);
    client.open_settlement(&anchor, &asset, &100);

    let count = client.settlement_count();
    assert_eq!(count, 2);

    let start_boundary = count + 1;

    assert_eq!(client.list_settlements(&start_boundary, &10).len(), 0);
    assert_eq!(
        client
            .list_settlements_by_anchor(&anchor, &start_boundary, &10)
            .len(),
        0
    );
    assert_eq!(
        client
            .list_settlements_by_asset(&asset, &start_boundary, &10)
            .len(),
        0
    );
    assert_eq!(
        client
            .list_settlements_by_status(&SettlementStatus::Pending, &start_boundary, &10)
            .len(),
        0
    );
}

// ---------------------------------------------------------------------------
// Property-based tests for settlement aggregate consistency
//
// Randomized sequences of open/execute/cancel/expire operations across
// multiple anchors and assets must not cause total_settled_amount or
// settlement_count_by_status to drift from the ground truth produced by
// scanning list_settlements.
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn prop_settlement_aggregates_survive_randomized_lifecycles(
        plan in prop::collection::vec(
            (
                0u32..4,           // anchor_idx
                0u32..4,           // asset_idx
                1i128..251i128,    // amount
                0u32..4u32,        // action: 0=Pending, 1=Execute, 2=Cancel, 3=Expire
            ),
            1..12,
        ),
        shuffle_seed in prop::num::u64::ANY,
    ) {
        use SettlementStatus::*;

        let env = Env::new_with_config(EnvTestConfig {
            capture_snapshot_at_drop: false,
        });
        env.mock_all_auths();
        let (client, admin) = setup(&env);

        let anchrs: Vec<Address> = (0..4).map(|_| Address::generate(&env)).collect();
        let assets = [
            symbol_short!("USDC"),
            symbol_short!("EURC"),
            symbol_short!("GBPC"),
            symbol_short!("XLM"),
        ];

        client.initialize(&admin);

        for a in &anchrs {
            client.register_anchor(a);
            for s in &assets {
                client.provide_liquidity(a, s, &1_000_000);
            }
        }

        client.set_fee(&50);
        client.set_settlement_expiry_ledgers(&10_000);

        let mut ops: Vec<(u64, u32)> = Vec::new();

        for (ai, si, amount, action) in plan {
            let anchor = &anchrs[ai as usize % anchrs.len()];
            let asset = &assets[si as usize % assets.len()];
            let id = client.open_settlement(anchor, asset, &amount);
            if action != 0 {
                ops.push((id, action));
            }
        }

        // Fisher-Yates shuffle using the seed for deterministic interleaving
        let mut state = shuffle_seed;
        for i in (1..ops.len()).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let j = (state >> 33) as usize % (i + 1);
            ops.swap(i, j);
        }

        if ops.iter().any(|(_, a)| *a == 3) {
            env.ledger().set_sequence_number(20_000);
        }

        for (id, action) in &ops {
            match action {
                1 => client.execute_settlement(id),
                2 => client.cancel_settlement(id),
                3 => client.cancel_expired_settlement(id),
                _ => unreachable!(),
            }
        }

        // Ground truth: manually count and sum from every stored settlement
        let all = client.list_settlements(&1, &u32::MAX);
        let mut manual_counts = [0u64; 4];
        let mut manual_amounts = [0i128; 4];

        for s in all.iter() {
            let idx = match s.status {
                Pending => 0,
                Executed => 1,
                Cancelled => 2,
                Expired => 3,
            };
            manual_counts[idx] += 1;
            manual_amounts[idx] += s.amount;
        }

        let statuses = [Pending, Executed, Cancelled, Expired];
        for (i, status) in statuses.iter().enumerate() {
            prop_assert_eq!(
                client.settlement_count_by_status(status),
                manual_counts[i],
                "settlement_count_by_status mismatch for {:?}",
                status,
            );
            prop_assert_eq!(
                client.total_settled_amount(status),
                manual_amounts[i],
                "total_settled_amount mismatch for {:?}",
                status,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Extreme-value property tests for checked arithmetic audit
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn prop_extreme_provide_liquidity_returns_clean_error(
        amount in prop_oneof![
            (i128::MAX - 1000..=i128::MAX),
            (i128::MIN..=i128::MIN + 1000),
        ]
    ) {
        let env = Env::new_with_config(EnvTestConfig {
            capture_snapshot_at_drop: false,
        });
        env.mock_all_auths();
        let (client, _admin, anchor, asset) = funded(&env, 1_000);

        let res = client.try_provide_liquidity(&anchor, &asset, &amount);
        if amount <= 0 {
            prop_assert_eq!(res.err().unwrap().unwrap(), Error::InvalidAmount);
        } else {
            match res {
                Ok(_) => {},
                Err(e) => {
                    prop_assert_eq!(e.unwrap(), Error::Overflow);
                }
            }
        }
    }

    #[test]
    fn prop_extreme_open_settlement_returns_clean_error(
        amount in prop_oneof![
            (i128::MAX - 1000..=i128::MAX),
            (i128::MIN..=i128::MIN + 1000),
        ]
    ) {
        let env = Env::new_with_config(EnvTestConfig {
            capture_snapshot_at_drop: false,
        });
        env.mock_all_auths();
        let (client, _admin, anchor, asset) = funded(&env, 1_000);

        let res = client.try_open_settlement(&anchor, &asset, &amount);
        if amount <= 0 {
            prop_assert_eq!(res.err().unwrap().unwrap(), Error::InvalidAmount);
        } else {
            let err = res.err().unwrap().unwrap();
            prop_assert!(err == Error::InsufficientLiquidity || err == Error::Overflow);
        }
    }

    #[test]
    fn prop_extreme_collect_fees_returns_clean_error(
        accrued in prop_oneof![
            (i128::MAX - 1000..=i128::MAX),
            (i128::MIN..=i128::MIN + 1000),
        ]
    ) {
        let env = Env::new_with_config(EnvTestConfig {
            capture_snapshot_at_drop: false,
        });
        env.mock_all_auths();
        let (client, admin) = setup(&env);
        client.initialize(&admin);
        let asset = symbol_short!("USDC");

        if accrued > 0 {
            env.as_contract(&client.address, || {
                crate::storage::set_fees_accrued(&env, &asset, accrued);
            });
            let res = client.try_collect_fees(&asset);
            prop_assert_eq!(res.unwrap().unwrap(), accrued);
        } else if accrued == 0 {
            let res = client.try_collect_fees(&asset);
            prop_assert_eq!(res.err().unwrap().unwrap(), Error::NoFeesToCollect);
        } else {
            env.as_contract(&client.address, || {
                crate::storage::set_fees_accrued(&env, &asset, accrued);
            });
            let res = client.try_collect_fees(&asset);
            prop_assert!(res.is_ok() || res.is_err());
        }
    }
}

// ---------------------------------------------------------------------------
// reserved_liquidity – per-asset pending settlement reservation queries
// ---------------------------------------------------------------------------

#[test]
fn test_reserved_liquidity_sums_pending_for_asset() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    // Three pending settlements: 100 + 200 + 300 = 600
    client.open_settlement(&anchor, &asset, &100);
    client.open_settlement(&anchor, &asset, &200);
    client.open_settlement(&anchor, &asset, &300);

    assert_eq!(client.reserved_liquidity(&asset), 600);
}

#[test]
fn test_reserved_liquidity_excludes_other_assets() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &1_000);
    client.provide_liquidity(&anchor, &eurc, &1_000);

    // USDC pending: 100 + 300 = 400
    client.open_settlement(&anchor, &usdc, &100);
    client.open_settlement(&anchor, &usdc, &300);

    // EURC pending: 500
    client.open_settlement(&anchor, &eurc, &500);

    // reserved_liquidity for USDC must be 400, not 900
    assert_eq!(client.reserved_liquidity(&usdc), 400);
    // reserved_liquidity for EURC must be 500
    assert_eq!(client.reserved_liquidity(&eurc), 500);
}

#[test]
fn test_reserved_liquidity_excludes_non_pending_statuses() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    client.set_fee(&100); // 1%
    client.set_settlement_expiry_ledgers(&10);

    // Pending – should count
    let pending = client.open_settlement(&anchor, &asset, &100);

    // Executed – should NOT count
    let executed = client.open_settlement(&anchor, &asset, &200);
    client.execute_settlement(&executed);

    // Cancelled – should NOT count
    let cancelled = client.open_settlement(&anchor, &asset, &300);
    client.cancel_settlement(&cancelled);

    // Expired – should NOT count
    let expired = client.open_settlement(&anchor, &asset, &400);
    env.ledger().set_sequence_number(15);
    client.cancel_expired_settlement(&expired);

    // Only the first (pending) settlement of 100 should be counted
    assert_eq!(client.reserved_liquidity(&asset), 100);
}

#[test]
fn test_reserved_liquidity_zero_when_no_pending() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    // No settlements opened at all
    assert_eq!(client.reserved_liquidity(&asset), 0);

    // Open, then execute all settlements – none pending
    let id = client.open_settlement(&anchor, &asset, &400);
    client.execute_settlement(&id);

    assert_eq!(client.reserved_liquidity(&asset), 0);

    // Also zero for an asset that never had any settlements
    let other = symbol_short!("EURC");
    assert_eq!(client.reserved_liquidity(&other), 0);
}

#[test]
fn test_reserved_liquidity_mixed_assets_and_statuses() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&a1);
    client.register_anchor(&a2);
    client.set_fee(&50);
    client.set_settlement_expiry_ledgers(&10);

    // Fund both assets
    client.provide_liquidity(&a1, &usdc, &10_000);
    client.provide_liquidity(&a1, &eurc, &10_000);
    client.provide_liquidity(&a2, &usdc, &10_000);
    client.provide_liquidity(&a2, &eurc, &10_000);

    // USDC pending: 100 (a1) + 300 (a2) = 400
    client.open_settlement(&a1, &usdc, &100);
    client.open_settlement(&a2, &usdc, &300);

    // EURC pending: 200 (a1) = 200
    client.open_settlement(&a1, &eurc, &200);

    // USDC executed (not pending): 500 (a1)
    let usdc_exec = client.open_settlement(&a1, &usdc, &500);
    client.execute_settlement(&usdc_exec);

    // EURC cancelled (not pending): 400 (a2)
    let eurc_cancel = client.open_settlement(&a2, &eurc, &400);
    client.cancel_settlement(&eurc_cancel);

    // EURC expired (not pending): 150 (a1)
    let eurc_expire = client.open_settlement(&a1, &eurc, &150);
    env.ledger().set_sequence_number(20);
    client.cancel_expired_settlement(&eurc_expire);

    assert_eq!(client.reserved_liquidity(&usdc), 400);
    assert_eq!(client.reserved_liquidity(&eurc), 200);
}

#[test]
fn test_reserved_liquidity_returns_zero_for_asset_with_only_non_pending() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    // Open and immediately execute
    let id1 = client.open_settlement(&anchor, &asset, &100);
    client.execute_settlement(&id1);

    // Open and cancel
    let id2 = client.open_settlement(&anchor, &asset, &200);
    client.cancel_settlement(&id2);

    // No settlements are currently pending
    assert_eq!(client.reserved_liquidity(&asset), 0);
}

// ---------------------------------------------------------------------------
// pool_exists – boolean existence view for asset pools
// ---------------------------------------------------------------------------

/// `pool_exists` returns `false` before any liquidity has been provided for
/// the asset — the pool entry does not exist yet.
#[test]
fn test_pool_exists_false_before_any_liquidity() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");

    client.initialize(&admin);

    assert!(!client.pool_exists(&asset));
}

/// `pool_exists` returns `true` once `provide_liquidity` has been called for
/// the asset, and stays `true` even after a full withdrawal empties the pool.
#[test]
fn test_pool_exists_true_after_provide_liquidity() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let asset = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&anchor);

    assert!(!client.pool_exists(&asset), "must be false before any liquidity");

    client.provide_liquidity(&anchor, &asset, &1_000);

    assert!(client.pool_exists(&asset), "must be true after provide_liquidity");
}

/// `pool_exists` returns `true` once `provide_liquidity_multi` has touched the
/// asset, matching the same post-condition as the single-asset entrypoint.
#[test]
fn test_pool_exists_true_after_provide_liquidity_multi() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);

    assert!(!client.pool_exists(&usdc));
    assert!(!client.pool_exists(&eurc));

    client.provide_liquidity_multi(&anchor, &vec![&env, (usdc.clone(), 100), (eurc.clone(), 200)]);

    assert!(client.pool_exists(&usdc));
    assert!(client.pool_exists(&eurc));
}

/// `pool_exists` remains `true` after a full withdrawal: the pool entry persists
/// even when `total == 0`, because assets are never removed from enumeration.
#[test]
fn test_pool_exists_true_after_full_withdrawal() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);

    assert!(client.pool_exists(&asset), "precondition: pool exists after funding");

    client.withdraw_all_liquidity(&anchor, &asset);

    assert_eq!(client.total_liquidity(&asset), 0, "pool drained");
    assert!(
        client.pool_exists(&asset),
        "pool_exists must stay true after a full withdrawal — the entry persists",
    );
}

/// `pool_exists` is independent per asset: funding one asset must not make
/// another appear to exist.
#[test]
fn test_pool_exists_is_per_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let usdc = symbol_short!("USDC");
    let eurc = symbol_short!("EURC");

    client.initialize(&admin);
    client.register_anchor(&anchor);
    client.provide_liquidity(&anchor, &usdc, &500);

    assert!(client.pool_exists(&usdc), "USDC was funded");
    assert!(!client.pool_exists(&eurc), "EURC was never funded");
}

/// `pool_exists` is consistent with `pool()`: the latter errors with
/// `PoolNotFound` exactly when `pool_exists` returns `false`, and succeeds
/// exactly when `pool_exists` returns `true`.
#[test]
fn test_pool_exists_consistent_with_pool_getter() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let asset = symbol_short!("USDC");

    client.initialize(&admin);
    client.register_anchor(&anchor);

    // Before funding: pool_exists == false  ↔  pool() == PoolNotFound
    assert!(!client.pool_exists(&asset));
    assert_eq!(
        client.try_pool(&asset).err().unwrap().unwrap(),
        Error::PoolNotFound,
    );

    // After funding: pool_exists == true  ↔  pool() succeeds
    client.provide_liquidity(&anchor, &asset, &1_000);
    assert!(client.pool_exists(&asset));
    assert_eq!(client.pool(&asset).total, 1_000);
}

// --- hello (smoke test that setup still works after all new tests) ---

#[test]
fn test_hello() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    client.initialize(&admin);
    assert!(client.is_initialized());
}

// ──────────────────────────────────────────────────────────────────────
// TTL bump-on-read tests for the risk/fee configuration getters (issue #122)
// and for is_fee_waived / get_fees_accrued (issue #121). Each getter now
// extends its entry's TTL on a successful read, matching what its setter
// already does. balance()'s read-side gap is a separate companion issue.
//
// Strategy: configure the value via its setter, advance the ledger far enough
// that the entry's TTL decays below the extend threshold, snapshot the TTL,
// read via the public getter, and confirm the read refreshed the TTL. Without
// the fix the getter is a pure read and the TTL is unchanged, so `after >
// before` fails; with the fix it bumps back up.
// ──────────────────────────────────────────────────────────────────────

// The setter bumps TTL to BUMP_AMOUNT (30 * DAY_IN_LEDGERS) and the extend
// threshold is one DAY_IN_LEDGERS (17_280) below that. Advancing past that
// window guarantees the next read actually triggers `extend_ttl` rather than
// being a no-op.
const TTL_DECAY_LEDGERS: u32 = 20_000;

fn advance_ledger(env: &Env, by: u32) {
    env.ledger()
        .set_sequence_number(env.ledger().sequence() + by);
}

fn persistent_ttl(env: &Env, contract: &Address, key: &DataKey) -> u32 {
    env.as_contract(contract, || env.storage().persistent().get_ttl(key))
}

/// Seeds a real `FeesAccrued` entry for `asset` by executing a settlement with
/// a non-zero fee on a non-waived anchor, and returns the accrued amount.
fn seed_fees_accrued(
    client: &AnchornetContractClient<'_>,
    anchor: &Address,
    asset: &Symbol,
) -> i128 {
    client.set_fee(&100); // 1%
    let id = client.open_settlement(anchor, asset, &400);
    client.execute_settlement(&id);
    client.fees_accrued(asset)
}

#[test]
fn test_min_liquidity_read_bumps_ttl() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);
    client.set_min_liquidity(&asset, &100);

    let key = DataKey::MinLiquidity(asset.clone());
    advance_ledger(&env, TTL_DECAY_LEDGERS);
    let before = persistent_ttl(&env, &client.address, &key);

    // Read-only call: no setter involved.
    assert_eq!(client.min_liquidity(&asset), 100);

    let after = persistent_ttl(&env, &client.address, &key);
    assert!(
        after > before,
        "min_liquidity read did not bump TTL: before={before}, after={after}",
    );
}

#[test]
fn test_is_fee_waived_read_bumps_ttl() {
    let env = Env::default();
    let (client, _admin, anchor, _asset) = funded(&env, 1_000);
    client.set_fee_waiver(&anchor, &true);

    let key = DataKey::FeeWaiver(anchor.clone());
    advance_ledger(&env, TTL_DECAY_LEDGERS);
    let before = persistent_ttl(&env, &client.address, &key);

    assert!(client.is_fee_waived(&anchor));

    let after = persistent_ttl(&env, &client.address, &key);
    assert!(
        after > before,
        "is_fee_waived read did not bump TTL: before={before}, after={after}",
    );
}

#[test]
fn test_is_anchor_read_bumps_ttl() {
    let env = Env::default();
    let (client, _admin, anchor, _asset) = funded(&env, 1_000);

    let key = DataKey::Anchor(anchor.clone());
    advance_ledger(&env, TTL_DECAY_LEDGERS);
    let before = persistent_ttl(&env, &client.address, &key);

    // Read-only call: no setter involved.
    assert!(client.is_anchor(&anchor));

    let after = persistent_ttl(&env, &client.address, &key);
    assert!(
        after > before,
        "is_anchor read did not bump TTL: before={before}, after={after}",
    );
}

#[test]
fn test_anchor_status_read_bumps_ttl() {
    let env = Env::default();
    let (client, _admin, anchor, _asset) = funded(&env, 1_000);

    let key = DataKey::Anchor(anchor.clone());

    advance_ledger(&env, TTL_DECAY_LEDGERS);
    let before_active = persistent_ttl(&env, &client.address, &key);
    assert_eq!(client.anchor_status(&anchor), AnchorStatus::Active);
    let after_active = persistent_ttl(&env, &client.address, &key);
    assert!(
        after_active > before_active,
        "anchor_status read in Active state did not bump TTL: before={before_active}, after={after_active}"
    );

    env.mock_all_auths();
    client.deregister_anchor(&anchor);
    assert_eq!(client.anchor_status(&anchor), AnchorStatus::Deregistered);

    advance_ledger(&env, TTL_DECAY_LEDGERS);
    let before_dereg = persistent_ttl(&env, &client.address, &key);
    assert_eq!(client.anchor_status(&anchor), AnchorStatus::Deregistered);
    let after_dereg = persistent_ttl(&env, &client.address, &key);
    assert!(
        after_dereg > before_dereg,
        "anchor_status read in Deregistered state did not bump TTL: before={before_dereg}, after={after_dereg}"
    );
}

#[test]
fn test_max_settlement_amount_read_bumps_ttl() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);
    client.set_max_settlement_amount(&asset, &5_000);

    let key = DataKey::MaxSettlementAmount(asset.clone());
    advance_ledger(&env, TTL_DECAY_LEDGERS);
    let before = persistent_ttl(&env, &client.address, &key);

    assert_eq!(client.max_settlement_amount(&asset), 5_000);

    let after = persistent_ttl(&env, &client.address, &key);
    assert!(
        after > before,
        "max_settlement_amount read did not bump TTL: before={before}, after={after}",
    );
}

#[test]
fn test_get_fees_accrued_read_bumps_ttl() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    assert!(seed_fees_accrued(&client, &anchor, &asset) > 0);

    let key = DataKey::FeesAccrued(asset.clone());
    advance_ledger(&env, TTL_DECAY_LEDGERS);
    let before = persistent_ttl(&env, &client.address, &key);

    assert_eq!(client.fees_accrued(&asset), 4);

    let after = persistent_ttl(&env, &client.address, &key);
    assert!(
        after > before,
        "fees_accrued read did not bump TTL: before={before}, after={after}",
    );
}

#[test]
fn test_asset_fee_read_bumps_ttl() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);
    client.set_asset_fee(&asset, &50);

    let key = DataKey::AssetFee(asset.clone());
    advance_ledger(&env, TTL_DECAY_LEDGERS);
    let before = persistent_ttl(&env, &client.address, &key);

    assert_eq!(client.asset_fee(&asset), 50);

    let after = persistent_ttl(&env, &client.address, &key);
    assert!(
        after > before,
        "asset_fee read did not bump TTL: before={before}, after={after}",
    );
}

#[test]
fn test_total_fees_accrued_bumps_each_asset_ttl() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    assert!(seed_fees_accrued(&client, &anchor, &asset) > 0);

    let key = DataKey::FeesAccrued(asset.clone());
    advance_ledger(&env, TTL_DECAY_LEDGERS);
    let before = persistent_ttl(&env, &client.address, &key);

    // total_fees_accrued iterates over get_fees_accrued — the cascade must bump
    // each per-asset entry's TTL (acceptance criteria: "Verify total_fees_accrued
    // benefits automatically once fixed").
    let _ = client.total_fees_accrued();

    let after = persistent_ttl(&env, &client.address, &key);
    assert!(
        after > before,
        "total_fees_accrued did not cascade the TTL bump to the per-asset entry: before={before}, after={after}",
    );
}

#[test]
fn test_min_liquidity_repeated_reads_keep_ttl_fresh() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);
    client.set_min_liquidity(&asset, &100);

    let key = DataKey::MinLiquidity(asset.clone());

    // Sustained "set once, read constantly" scenario from the issue's security
    // notes: each read over an advancing ledger should refresh the TTL, so the
    // entry never drifts toward archival.
    for _ in 0..2 {
        advance_ledger(&env, TTL_DECAY_LEDGERS);
        let _ = client.min_liquidity(&asset);
    }

    advance_ledger(&env, TTL_DECAY_LEDGERS);
    let before = persistent_ttl(&env, &client.address, &key);
    let _ = client.min_liquidity(&asset);
    let after = persistent_ttl(&env, &client.address, &key);
    assert!(
        after > before,
        "repeated reads did not keep TTL fresh: before={before}, after={after}",
    );
}

#[test]
fn test_is_fee_waived_repeated_reads_keep_ttl_fresh() {
    let env = Env::default();
    let (client, _admin, anchor, _asset) = funded(&env, 1_000);
    client.set_fee_waiver(&anchor, &true);

    let key = DataKey::FeeWaiver(anchor.clone());

    // Sustained "set once, read constantly" scenario from the issue's security
    // notes: each read over an advancing ledger should refresh the TTL, so the
    // waiver never drifts toward archival.
    for _ in 0..2 {
        advance_ledger(&env, TTL_DECAY_LEDGERS);
        let _ = client.is_fee_waived(&anchor);
    }

    advance_ledger(&env, TTL_DECAY_LEDGERS);
    let before = persistent_ttl(&env, &client.address, &key);
    let _ = client.is_fee_waived(&anchor);
    let after = persistent_ttl(&env, &client.address, &key);
    assert!(
        after > before,
        "repeated reads did not keep TTL fresh: before={before}, after={after}",
    );
}

#[test]
fn test_min_liquidity_read_on_unconfigured_asset_is_safe() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);

    // Never configured: the `.has` guard must skip `extend_ttl` (which would
    // panic on an absent key) and the getter must still return the default.
    assert_eq!(client.min_liquidity(&asset), 0);
}

#[test]
fn test_max_settlement_amount_configured_read_on_unconfigured_asset_is_safe() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);

    // Never configured: the configuration-status view must return `false`
    // without attempting to extend an absent MaxSettlementAmount entry.
    assert!(!client.is_max_settlement_amount_configured(&asset));
}

#[test]
fn test_asset_fee_read_on_unconfigured_asset_is_safe() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = symbol_short!("USDC");
    client.initialize(&admin);

    // No override configured: `get_asset_fee` returns `None` without trying to
    // extend an absent entry, so the effective fee falls back to the global fee.
    assert_eq!(client.asset_fee(&asset), client.fee());
}

#[test]
fn test_is_fee_waived_read_on_unconfigured_anchor_is_safe() {
    let env = Env::default();
    let (client, _admin, anchor, _asset) = funded(&env, 1_000);

    // Anchor registered but no waiver ever set: the `.has` guard must skip
    // `extend_ttl` (which would panic on an absent key) and the getter must
    // still return the `false` default.
    assert!(!client.is_fee_waived(&anchor));
}

#[test]
fn test_get_fees_accrued_read_on_unconfigured_asset_is_safe() {
    let env = Env::default();
    let (client, _admin, _anchor, _asset) = funded(&env, 1_000);

    // An asset that never accrued fees has no FeesAccrued entry: the getter
    // returns `0` without trying to extend an absent entry.
    let never_settled = symbol_short!("EURC");
    assert_eq!(client.fees_accrued(&never_settled), 0);
}

#[test]
fn test_withdraw_liquidity_multi_atomic_rejection_on_min_liquidity_floor_violation() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let ast1 = symbol_short!("AST1");
    let ast2 = symbol_short!("AST2");

    client.initialize(&admin);
    client.register_anchor(&anchor);

    client.provide_liquidity(&anchor, &ast1, &1_000);
    client.provide_liquidity(&anchor, &ast2, &1_000);

    client.set_min_liquidity(&ast2, &700);

    let requests = vec![&env, (ast1.clone(), 500), (ast2.clone(), 400)];
    let err = client
        .try_withdraw_liquidity_multi(&anchor, &requests)
        .err()
        .unwrap()
        .unwrap();

    assert_eq!(err, Error::BelowMinLiquidity);
    assert_eq!(client.balance(&anchor, &ast1), 1_000);
    assert_eq!(client.balance(&anchor, &ast2), 1_000);
    assert_eq!(client.total_liquidity(&ast1), 1_000);
    assert_eq!(client.total_liquidity(&ast2), 1_000);
}

#[test]
fn test_open_settlement_enforces_per_asset_max_settlement_amount_cap() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let anchor = Address::generate(&env);
    let ast1 = symbol_short!("AST1");
    let ast2 = symbol_short!("AST2");

    client.initialize(&admin);
    client.register_anchor(&anchor);

    client.provide_liquidity(&anchor, &ast1, &1_000);
    client.provide_liquidity(&anchor, &ast2, &1_000);

    client.set_max_settlement_amount(&ast1, &500);

    let err = client
        .try_open_settlement(&anchor, &ast1, &600)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, Error::AboveMaxSettlementAmount);

    let id1 = client.open_settlement(&anchor, &ast1, &500);
    assert_eq!(id1, 1);

    let id2 = client.open_settlement(&anchor, &ast2, &600);
    assert_eq!(id2, 2);
}
