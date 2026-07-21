# Contract error reference

The AnchorNet contract exposes the `Error` enum as a set of stable `u32`
Soroban contract error codes. Integrators can use this reference to identify
the condition represented by each code and the public contract entrypoints
that may return it.

The mapping below was verified against executable `Error::Variant` return
sites, `ok_or(...)` conversions, and `?` propagation through private
validation helpers in `src/lib.rs`. Only public contract entrypoints are
listed; private helpers are not part of the contract interface.

Authorization failures raised by Soroban's `Address::require_auth` are host
authorization errors and are separate from the contract `Error` variants
listed here.

| Code | Variant | Condition | Public entrypoints |
|---:|---|---|---|
| 1 | `AlreadyInitialized` | The contract has already been initialized and cannot be initialized again. | `initialize` |
| 2 | `NotInitialized` | The requested operation requires an initialized administrator, but no administrator has been stored yet. | `admin`<br>`set_admin`<br>`propose_admin`<br>`set_operator`<br>`pause`<br>`unpause`<br>`extend_instance_ttl`<br>`set_fee`<br>`set_fee_waiver`<br>`set_asset_fee`<br>`clear_asset_fee`<br>`set_settlement_expiry_ledgers`<br>`collect_fees`<br>`register_anchor`<br>`register_anchors`<br>`deregister_anchor`<br>`set_min_liquidity`<br>`set_max_settlement_amount`<br>`execute_settlement` |
| 3 | `NotAuthorized` | The explicit caller of an admin-or-operator lifecycle action is neither the current administrator nor the appointed operator. | `pause`<br>`unpause`<br>`extend_instance_ttl` |
| 4 | `AnchorAlreadyRegistered` | An anchor is already registered, or the same address appears more than once in a batch registration. | `register_anchor`<br>`register_anchors` |
| 5 | `AnchorNotRegistered` | The supplied anchor or provider is not currently registered for an operation that requires registration. | `set_fee_waiver`<br>`deregister_anchor`<br>`provide_liquidity`<br>`provide_liquidity_multi`<br>`open_settlement` |
| 6 | `InvalidAmount` | An amount-related input is invalid: a required operation amount is non-positive, a batch is empty or contains a non-positive amount, or a configured floor or cap is negative. | `quote_fee`<br>`provide_liquidity`<br>`provide_liquidity_multi`<br>`set_min_liquidity`<br>`set_max_settlement_amount`<br>`withdraw_liquidity`<br>`withdraw_liquidity_multi`<br>`open_settlement` |
| 7 | `InsufficientLiquidity` | A provider balance or pool total is insufficient for the requested withdrawal or settlement reservation. | `withdraw_liquidity`<br>`withdraw_liquidity_multi`<br>`withdraw_all_liquidity`<br>`open_settlement` |
| 8 | `PoolNotFound` | No liquidity pool exists for the requested asset. | `pool` |
| 9 | `Paused` | A liquidity or settlement mutation was attempted while the contract was paused. | `provide_liquidity`<br>`provide_liquidity_multi`<br>`withdraw_liquidity`<br>`withdraw_liquidity_multi`<br>`withdraw_all_liquidity`<br>`open_settlement` |
| 10 | `InvalidFee` | The supplied fee in basis points exceeds the maximum accepted by the contract. | `set_fee`<br>`set_asset_fee` |
| 11 | `SettlementNotFound` | No settlement exists with the supplied settlement identifier. | `execute_settlement`<br>`cancel_settlement`<br>`cancel_expired_settlement`<br>`is_settlement_expired`<br>`settlement` |
| 12 | `InvalidSettlementState` | The settlement is not pending and therefore cannot undergo the requested execution, cancellation, or expiry-reclamation transition. | `execute_settlement`<br>`cancel_settlement`<br>`cancel_expired_settlement` |
| 13 | `NoFeesToCollect` | The requested asset has no accrued protocol fees available for collection. | `collect_fees` |
| 14 | `NoPendingAdmin` | No administrator transfer proposal is currently stored. | `pending_admin`<br>`accept_admin` |
| 15 | `NotPendingAdmin` | The supplied candidate is not the address currently proposed as the next administrator. | `accept_admin` |
| 16 | `SettlementNotExpired` | Settlement expiry is disabled or the pending settlement has not yet reached its expiry ledger. | `cancel_expired_settlement` |
| 17 | `BelowMinLiquidity` | The requested withdrawal would leave the asset pool below its configured minimum liquidity floor. | `withdraw_liquidity`<br>`withdraw_liquidity_multi`<br>`withdraw_all_liquidity` |
| 18 | `NoOperator` | No lifecycle operator has been appointed. | `operator` |
| 19 | `AboveMaxSettlementAmount` | The requested settlement amount exceeds the enabled per-asset maximum settlement amount. | `open_settlement` |
| 20 | `DuplicateAssetInBatch` | The same asset appears more than once in a batch liquidity operation. | `provide_liquidity_multi`<br>`withdraw_liquidity_multi` |
| 21 | `InvalidAdminCandidate` | The proposed administrator is the same as the current administrator. | `propose_admin` |

## Maintaining this reference

When adding, removing, or changing an `Error` variant:

1. Update `src/error.rs` without reusing an existing numeric code.
2. Search executable `src/lib.rs` code for every direct return site.
3. Trace errors propagated with `?` through private helpers to their public
   entrypoints.
4. Update this reference and verify that every declared variant appears
   exactly once.
