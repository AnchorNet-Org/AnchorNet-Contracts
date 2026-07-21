use crate::{AnchornetContract, AnchornetContractClient, Error, SettlementStatus};
use proptest::prelude::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, EnvTestConfig, Events as _, Ledger as _},
    vec, Address, Env, IntoVal, Symbol,
};

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
                (symbol_short!("admin"),).into_val(&env),
                new_admin.into_val(&env),
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

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
    // The pool total already sits at `i128::MAX`; adding any further
    // liquidity must overflow rather than silently wrap, relying on the
    // crate-wide `overflow-checks = true` guarantee.
    client.provide_liquidity(&anchor, &usdc, &i128::MAX);
    client.provide_liquidity(&anchor, &usdc, &1);
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
}

#[test]
fn test_set_settlement_expiry_ledgers_updates_value() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    client.initialize(&admin);

    client.set_settlement_expiry_ledgers(&100);

    assert_eq!(client.settlement_expiry_ledgers(), 100);
}

#[test]
fn test_cancel_expired_settlement_disabled_by_default() {
    let env = Env::default();
    let (client, _admin, anchor, asset) = funded(&env, 1_000);
    let id = client.open_settlement(&anchor, &asset, &400);

    // Expiry is disabled (zero) by default, no matter how far the ledger
    // advances.
    env.ledger().set_sequence_number(1_000_000);
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
#[should_panic]
fn test_operator_cannot_set_fee() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let operator = Address::generate(&env);
    client.initialize(&admin);
    client.set_operator(&operator);

    // The operator role only extends to pause/unpause; admin-only mutations
    // like `set_fee` still require the real admin's own signature, which is
    // unaffected by an operator being appointed. With every mocked auth
    // cleared, `set_fee` has no way to authenticate and must panic rather
    // than let the operator (or anyone else) stand in for the admin.
    env.set_auths(&[]);
    client.set_fee(&25);
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

    // With no floor configured, a full withdrawal is unaffected.
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

    // With no cap configured, a large settlement is unaffected.
    client.open_settlement(&anchor, &asset, &1_000);
}

#[test]
fn test_set_max_settlement_amount_updates_value() {
    let env = Env::default();
    let (client, _admin, _anchor, asset) = funded(&env, 1_000);

    client.set_max_settlement_amount(&asset, &500);

    assert_eq!(client.max_settlement_amount(&asset), 500);
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

// --- hello (smoke test that setup still works after all new tests) ---

#[test]
fn test_hello() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    client.initialize(&admin);
    assert!(client.is_initialized());
}
