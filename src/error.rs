//! Error types returned by the AnchorNet contract.
//!
//! See the [contract error reference](../docs/ERRORS.md) for the stable
//! error codes, their conditions, and their originating public entrypoints.

use soroban_sdk::contracterror;

/// Errors that can be returned by the AnchorNet liquidity contract.
///
/// Each variant maps to a stable `u32` code so that off-chain clients can
/// match on the error regardless of SDK version.
///
/// See the [contract error reference](../docs/ERRORS.md) for the condition
/// and public entrypoints associated with every variant.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The contract has already been initialized.
    AlreadyInitialized = 1,
    /// The contract has not been initialized yet.
    NotInitialized = 2,
    /// The caller is not authorized to perform this action.
    NotAuthorized = 3,
    /// The anchor is already registered.
    AnchorAlreadyRegistered = 4,
    /// The anchor is not registered.
    AnchorNotRegistered = 5,
    /// The supplied amount must be greater than zero.
    InvalidAmount = 6,
    /// The pool does not hold enough liquidity for this operation.
    InsufficientLiquidity = 7,
    /// No pool exists for the requested asset.
    PoolNotFound = 8,
    /// The contract is paused and cannot process this operation.
    Paused = 9,
    /// The fee value is outside the allowed range.
    InvalidFee = 10,
    /// No settlement exists with the requested id.
    SettlementNotFound = 11,
    /// The settlement is not in a state that allows this transition.
    InvalidSettlementState = 12,
    /// There are no accrued fees to collect.
    NoFeesToCollect = 13,
    /// No admin transfer has been proposed.
    NoPendingAdmin = 14,
    /// The caller is not the address proposed as the next administrator.
    NotPendingAdmin = 15,
    /// The settlement has not yet reached its expiry ledger, or expiry is
    /// disabled, so it cannot be reclaimed via `cancel_expired_settlement`.
    SettlementNotExpired = 16,
    /// The withdrawal would leave the pool below its configured minimum
    /// liquidity floor.
    BelowMinLiquidity = 17,
    /// No operator has been appointed.
    NoOperator = 18,
    /// The settlement amount exceeds the configured per-asset maximum.
    AboveMaxSettlementAmount = 19,
    /// The same asset appeared more than once in a batch operation.
    DuplicateAssetInBatch = 20,
    /// The proposed administrator is the same as the current administrator.
    InvalidAdminCandidate = 21,
    /// Arithmetic overflow or underflow occurred.
    Overflow = 22,
}
