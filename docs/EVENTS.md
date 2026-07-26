# AnchorNet Event Reference

This document catalogs every event topic emitted by the AnchorNet contract, including argument types, emitting entrypoints, and examples of the raw event structure as Soroban emits it.

Indexers subscribe to these events to keep an off-chain view of pool liquidity in sync with on-chain state. All events are emitted by functions in [`src/events.rs`](../src/events.rs) and invoked from [`src/lib.rs`](../src/lib.rs).

## Event Structure

Soroban events are composed of:
- **Topics**: a tuple of indexed values (SearchableByTopics in the event header)
- **Data**: a single unindexed payload value

Both topics and data are Soroban SDK types (e.g., `Address`, `Symbol`, `u32`, `i128`, `u64`, etc.).

---

## Event Catalog

### 1. **init** — Contract Initialization

| Property | Value |
|----------|-------|
| **Topics** | `("init",)` |
| **Data** | `Address` (admin) |
| **Emitted by** | [`initialize`](../src/lib.rs#L41) |
| **Emitted from** | [`initialized`](../src/events.rs#L25) |

Emitted once when the contract is initialized. The data contains the address of the initial administrator.

**Raw Event Example:**
```rust
topics = ("init",)
data = Address { account_id: "..." }
```

---

### 2. **admin** — Administrator Changed

| Property | Value |
|----------|-------|
| **Topics** | `("admin", path)` where `path` is `"direct"` or `"accept"` |
| **Data** | `Address` (new_admin) |
| **Emitted by** | [`set_admin`](../src/lib.rs#L70) or [`accept_admin`](../src/lib.rs#L109) |
| **Emitted from** | [`admin_changed`](../src/events.rs#L32) |

Emitted when the administrator is transferred. The second topic indicates the transfer method:
- `"direct"` — single-step transfer via `set_admin`
- `"accept"` — two-step transfer accepted via `accept_admin`

**Raw Event Examples:**

Single-step transfer:
```rust
topics = ("admin", "direct")
data = Address { account_id: "..." }
```

Two-step accepted transfer:
```rust
topics = ("admin", "accept")
data = Address { account_id: "..." }
```

---

### 3. **propose** — Admin Transfer Proposed

| Property | Value |
|----------|-------|
| **Topics** | `("propose",)` |
| **Data** | `Address` (candidate) |
| **Emitted by** | [`propose_admin`](../src/lib.rs#L87) |
| **Emitted from** | [`admin_proposed`](../src/events.rs#L43) |

Emitted when an admin transfer is proposed via the two-step flow. The data contains the proposed next administrator.

**Raw Event Example:**
```rust
topics = ("propose",)
data = Address { account_id: "..." }
```

---

### 4. **anchor** — Anchor Registered

| Property | Value |
|----------|-------|
| **Topics** | `("anchor", anchor)` |
| **Data** | `()` (unit) |
| **Emitted by** | [`register_anchor`](../src/lib.rs#L358) or [`register_anchors`](../src/lib.rs#L376) |
| **Emitted from** | [`anchor_registered`](../src/events.rs#L49) |

Emitted when an anchor is registered as an approved liquidity provider. The second topic is the registered anchor's address.

**Raw Event Example:**
```rust
topics = ("anchor", Address { account_id: "..." })
data = ()
```

---

### 5. **deanchor** — Anchor Deregistered

| Property | Value |
|----------|-------|
| **Topics** | `("deanchor", anchor)` |
| **Data** | `()` (unit) |
| **Emitted by** | [`deregister_anchor`](../src/lib.rs#L433) |
| **Emitted from** | [`anchor_removed`](../src/events.rs#L93) |

Emitted when an anchor is removed from the approved set. Existing pool liquidity is unaffected. The second topic is the deregistered anchor's address.

**Raw Event Example:**
```rust
topics = ("deanchor", Address { account_id: "..." })
data = ()
```

---

### 6. **provide** — Liquidity Provided

| Property | Value |
|----------|-------|
| **Topics** | `("provide", provider, asset)` |
| **Data** | `i128` (amount) |
| **Emitted by** | [`provide_liquidity`](../src/lib.rs#L447) or [`provide_liquidity_multi`](../src/lib.rs#L470) |
| **Emitted from** | [`liquidity_provided`](../src/events.rs#L55) |

Emitted when an anchor provides liquidity to a pool. Topics include the provider address and asset symbol; data is the amount provided.

**Raw Event Example:**
```rust
topics = ("provide", Address { account_id: "..." }, Symbol::short("USDC"))
data = 1_000_000  // 1 USDC (assuming 6 decimals)
```

---

### 7. **onboarded** — Asset Onboarded

| Property | Value |
|----------|-------|
| **Topics** | `("onboarded", asset)` |
| **Data** | `()` (unit) |
| **Emitted by** | [`provide_liquidity`](../src/lib.rs#L447) or [`provide_liquidity_multi`](../src/lib.rs#L470) (indirectly via `do_provide`) |
| **Emitted from** | [`asset_onboarded`](../src/events.rs#L63) |

Emitted when an asset receives liquidity for the first time and is onboarded to the contract. The second topic is the asset symbol.

**Raw Event Example:**
```rust
topics = ("onboarded", Symbol::short("USDC"))
data = ()
```

---

### 8. **withdraw** — Liquidity Withdrawn

| Property | Value |
|----------|-------|
| **Topics** | `("withdraw", provider, asset)` |
| **Data** | `i128` (amount) |
| **Emitted by** | [`withdraw_liquidity`](../src/lib.rs#L549), [`withdraw_all_liquidity`](../src/lib.rs#L624), or [`withdraw_liquidity_multi`](../src/lib.rs#L578) |
| **Emitted from** | [`liquidity_withdrawn`](../src/events.rs#L75) |

Emitted when an anchor withdraws liquidity from a pool. Topics include the provider address and asset symbol; data is the amount withdrawn.

**Note:** Both `withdraw_liquidity` and `withdraw_all_liquidity` emit this event via the same internal code path (`do_withdraw`), guaranteeing identical topic/data shapes for equivalent withdrawals. This parity is enforced by a regression test (`test_withdraw_event_parity` in `src/test.rs`).

**Raw Event Example:**
```rust
topics = ("withdraw", Address { account_id: "..." }, Symbol::short("USDC"))
data = 500_000  // 0.5 USDC
```

---

### 9. **paused** — Paused Flag Changed

| Property | Value |
|----------|-------|
| **Topics** | `("paused",)` |
| **Data** | `bool` (paused) |
| **Emitted by** | [`pause`](../src/lib.rs#L163) or [`unpause`](../src/lib.rs#L173) |
| **Emitted from** | [`paused_changed`](../src/events.rs#L83) |

Emitted when the contract is paused or unpaused. Data is `true` for pause, `false` for unpause.

**Raw Event Examples:**

Pause:
```rust
topics = ("paused",)
data = true
```

Unpause:
```rust
topics = ("paused",)
data = false
```

---

### 10. **fee** — Global Protocol Fee Changed

| Property | Value |
|----------|-------|
| **Topics** | `("fee",)` |
| **Data** | `u32` (bps — basis points) |
| **Emitted by** | [`set_fee`](../src/lib.rs#L196) |
| **Emitted from** | [`fee_changed`](../src/events.rs#L88) |

Emitted when the global protocol fee is updated. Data is the new fee rate in basis points (max 1000 = 10%).

**Raw Event Example:**
```rust
topics = ("fee",)
data = 50  // 0.5% (50 bps)
```

---

### 11. **waiver** — Anchor Fee Waiver Changed

| Property | Value |
|----------|-------|
| **Topics** | `("waiver", anchor)` |
| **Data** | `bool` (waived) |
| **Emitted by** | [`set_fee_waiver`](../src/lib.rs#L235) |
| **Emitted from** | [`fee_waiver_changed`](../src/events.rs#L116) |

Emitted when an anchor is granted or revoked a protocol fee waiver. Data is `true` for waived, `false` for revoked.

**Raw Event Examples:**

Grant waiver:
```rust
topics = ("waiver", Address { account_id: "..." })
data = true
```

Revoke waiver:
```rust
topics = ("waiver", Address { account_id: "..." })
data = false
```

---

### 12. **assetfee** — Asset-Specific Fee Override Set

| Property | Value |
|----------|-------|
| **Topics** | `("assetfee", asset)` |
| **Data** | `u32` (bps — basis points) |
| **Emitted by** | [`set_asset_fee`](../src/lib.rs#L254) |
| **Emitted from** | [`asset_fee_changed`](../src/events.rs#L166) |

Emitted when an asset-specific fee override is set, independent of the global rate. Data is the override fee rate in basis points.

**Raw Event Example:**
```rust
topics = ("assetfee", Symbol::short("BTC"))
data = 100  // 1% (100 bps) for this asset only
```

---

### 13. **feeclear** — Asset Fee Override Cleared

| Property | Value |
|----------|-------|
| **Topics** | `("feeclear", asset)` |
| **Data** | `()` (unit) |
| **Emitted by** | [`clear_asset_fee`](../src/lib.rs#L266) |
| **Emitted from** | [`asset_fee_cleared`](../src/events.rs#L173) |

Emitted when an asset's fee override is cleared, reverting it to the global fee. The second topic is the asset symbol.

**Raw Event Example:**
```rust
topics = ("feeclear", Symbol::short("BTC"))
data = ()
```

---

### 14. **settle** — Settlement Opened

| Property | Value |
|----------|-------|
| **Topics** | `("settle", anchor, asset)` |
| **Data** | `u64` (settlement_id) |
| **Emitted by** | [`open_settlement`](../src/lib.rs#L640) |
| **Emitted from** | [`settlement_opened`](../src/events.rs#L99) |

Emitted when a settlement is opened, reserving pool liquidity. Topics include the anchor that opened it and the asset being settled; data is the newly assigned settlement ID.

**Raw Event Example:**
```rust
topics = ("settle", Address { account_id: "..." }, Symbol::short("USDC"))
data = 1  // settlement ID
```

---

### 15. **executed** — Settlement Executed

| Property | Value |
|----------|-------|
| **Topics** | `("executed", id)` |
| **Data** | `()` (unit) |
| **Emitted by** | [`execute_settlement`](../src/lib.rs#L700) |
| **Emitted from** | [`settlement_executed`](../src/events.rs#L105) |

Emitted when a pending settlement is finalized. The reserved liquidity is considered released to the anchor, and its fee is accrued. The second topic is the settlement ID.

**Raw Event Example:**
```rust
topics = ("executed", 1)
data = ()
```

---

### 16. **cancelled** — Settlement Cancelled

| Property | Value |
|----------|-------|
| **Topics** | `("cancelled", id)` |
| **Data** | `()` (unit) |
| **Emitted by** | [`cancel_settlement`](../src/lib.rs#L720) |
| **Emitted from** | [`settlement_cancelled`](../src/events.rs#L110) |

Emitted when a pending settlement is cancelled. The reserved liquidity is returned to the pool. The second topic is the settlement ID.

**Raw Event Example:**
```rust
topics = ("cancelled", 1)
data = ()
```

---

### 17. **expired** — Settlement Expired and Reclaimed

| Property | Value |
|----------|-------|
| **Topics** | `("expired", id)` |
| **Data** | `()` (unit) |
| **Emitted by** | [`cancel_expired_settlement`](../src/lib.rs#L760) |
| **Emitted from** | [`settlement_expired`](../src/events.rs#L135) |

Emitted when a pending settlement is reclaimed after exceeding the configured expiry window. The reserved liquidity is returned to the pool. The second topic is the settlement ID. This entrypoint requires no authorization; anyone may call it.

**Raw Event Example:**
```rust
topics = ("expired", 1)
data = ()
```

---

### 18. **expiry** — Settlement Expiry Window Changed

| Property | Value |
|----------|-------|
| **Topics** | `("expiry",)` |
| **Data** | `u32` (ledgers) |
| **Emitted by** | [`set_settlement_expiry_ledgers`](../src/lib.rs#L294) |
| **Emitted from** | [`settlement_expiry_changed`](../src/events.rs#L129) |

Emitted when the settlement expiry window is updated. Data is the new window in ledgers (zero disables expiry).

**Raw Event Example:**
```rust
topics = ("expiry",)
data = 86_400  // ~6 days of ledgers (assuming 10-second blocks)
```

---

### 19. **collect** — Fees Collected

| Property | Value |
|----------|-------|
| **Topics** | `("collect", asset)` |
| **Data** | `i128` (amount) |
| **Emitted by** | [`collect_fees`](../src/lib.rs#L343) |
| **Emitted from** | [`fees_collected`](../src/events.rs#L122) |

Emitted when accrued protocol fees for an asset are collected and the balance is reset to zero. Topics include the asset symbol; data is the collected amount.

**Raw Event Example:**
```rust
topics = ("collect", Symbol::short("USDC"))
data = 50_000  // 0.05 USDC (50,000 stroops)
```

---

### 20. **minliq** — Minimum Liquidity Floor Changed

| Property | Value |
|----------|-------|
| **Topics** | `("minliq", asset)` |
| **Data** | `i128` (floor) |
| **Emitted by** | [`set_min_liquidity`](../src/lib.rs#L507) |
| **Emitted from** | [`min_liquidity_changed`](../src/events.rs#L141) |

Emitted when the minimum liquidity floor for an asset is configured. The floor prevents the pool from being withdrawn below this level. Topics include the asset symbol; data is the new floor value (zero disables).

**Raw Event Example:**
```rust
topics = ("minliq", Symbol::short("USDC"))
data = 1_000_000_000  // 1,000 USDC (assuming 6 decimals)
```

---

### 21. **maxamt** — Maximum Settlement Amount Changed

| Property | Value |
|----------|-------|
| **Topics** | `("maxamt", asset)` |
| **Data** | `i128` (amount) |
| **Emitted by** | [`set_max_settlement_amount`](../src/lib.rs#L527) |
| **Emitted from** | [`max_settlement_amount_changed`](../src/events.rs#L159) |

Emitted when the maximum settlement amount for an asset is configured. This caps the amount a single `open_settlement` call may reserve. Topics include the asset symbol; data is the new cap (zero disables).

**Raw Event Example:**
```rust
topics = ("maxamt", Symbol::short("BTC"))
data = 100_000_000  // 1 BTC (assuming 8 decimals)
```

---

### 22. **operator** — Operator Appointed

| Property | Value |
|----------|-------|
| **Topics** | `("operator",)` |
| **Data** | `Address` (operator) |
| **Emitted by** | [`set_operator`](../src/lib.rs#L129) |
| **Emitted from** | [`operator_changed`](../src/events.rs#L147) |

Emitted when an operator is appointed. The operator may pause/unpause but cannot change fees, admin, or other privileged settings. Data is the operator's address.

**Raw Event Example:**
```rust
topics = ("operator",)
data = Address { account_id: "..." }
```

---

### 23. **op_clear** — Operator Role Revoked

| Property | Value |
|----------|-------|
| **Topics** | `("op_clear",)` |
| **Data** | `()` (unit) |
| **Emitted by** | [`clear_operator`](../src/lib.rs#L139) |
| **Emitted from** | [`operator_cleared`](../src/events.rs#L153) |

Emitted when the operator role is revoked, returning the contract to an operator-less state.

**Raw Event Example:**
```rust
topics = ("op_clear",)
data = ()
```

---

## Adding New Events

When adding a new event:

1. **Define the event function** in `src/events.rs` following the existing pattern:
   - Name it `event_name()` and document topics/data in the doc comment
   - Use `symbol_short!()` for topic symbols
   - Emit via `env.events().publish(topics, data)`

2. **Call it from the corresponding entrypoint** in `src/lib.rs` exactly once per logical operation

3. **Update this document** with a new section following the template above, including:
   - Topics structure (all indices in the tuple)
   - Data type and value
   - Emitting entrypoint and helper function
   - A raw event example

4. **Verify parity** if the event is emitted from multiple paths (see `test_withdraw_event_parity` in `src/test.rs` for a reference implementation)

---

## Indexer Integration

Off-chain indexers subscribe to these topics and maintain a replicated view of pool state. For each event:
- Parse the topic tuple to identify event type and parameters
- Extract the data payload
- Update off-chain state accordingly

Topics are indexed and searchable; data is always unindexed. This structure allows efficient filtering by, e.g., `("provide", provider_address, asset_symbol)` without scanning all event data.

