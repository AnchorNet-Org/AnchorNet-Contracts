# Pagination Semantics

This document catalogues the start and limit semantics across every paginated entrypoint in the AnchorNet contract. The contract provides ten distinct `list_*/paginated` entrypoints.

There are two primary paradigms for pagination in the contract:
1. **Index-based Pagination:** The `start` parameter is a 0-based list index.
2. **ID-based Pagination:** The `start` parameter is a 1-based sequential ID.

Both paradigms support filtering, where non-matching entries are skipped without counting toward the `limit`.

## Entrypoints Catalog

### Index-based Entrypoints
These entrypoints scan a persistent underlying list starting from a 0-based list index. The `start` parameter is a `u32` index.

- `list_anchors(start: u32, limit: u32)`: Pages through currently registered anchors. Deregistered anchors are skipped without counting toward `limit`.
- `list_fee_waived_anchors(start: u32, limit: u32)`: Pages through registered anchors, filtering for those with an active fee waiver. Non-matching/deregistered anchors are skipped without counting.
- `list_assets(start: u32, limit: u32)`: Pages through every asset that has ever had liquidity provided, without filtering until `limit` is reached.
- `anchor_balances(provider: Address, start: u32, limit: u32)`: Pages through the known assets list, returning non-zero balances for a provider. Assets with zero balance are skipped without counting.

### ID-based Entrypoints
These entrypoints iterate over settlements using a 1-based sequential ID. The `start` parameter is a `u64` settlement ID. If `start` is 0, it behaves identically to `start` being 1. Missing IDs (e.g. if skipped internally) and non-matching entries do not count toward the `limit`.

- `list_settlements(start: u64, limit: u32)`: Pages through all settlements. Missing IDs are skipped without counting.
- `list_settlements_by_anchor(anchor: Address, start: u64, limit: u32)`: Pages through settlements opened by `anchor`. Missing or non-matching IDs are skipped without counting.
- `list_settlements_by_asset(asset: Symbol, start: u64, limit: u32)`: Pages through settlements in `asset`. Missing or non-matching IDs are skipped without counting.
- `list_settlements_by_status(status: SettlementStatus, start: u64, limit: u32)`: Pages through settlements matching `status`. Missing or non-matching IDs are skipped without counting.

> **Note:** If additional compound filters are added to the contract, they will inherit the same ID-based start semantics and skip-without-counting behavior as the existing settlement filters.

## Skip-without-counting Behavior

For all filtered variants (e.g. `list_fee_waived_anchors`, `list_settlements_by_anchor`, etc.), the underlying list or ID sequence is scanned until `limit` *matching* entries are accumulated or the end of the sequence is reached. Skipped items do not decrement the `limit` budget. This means a caller asking for `limit = 10` will always receive up to 10 matching entries, regardless of how many non-matching entries reside between them.

Off-chain clients that mishandle the id-based vs. index-based distinction, or that miscount skipped entries against limit, risk building an incomplete or duplicated view of anchors, assets, or settlements.

## Worked Examples

### Example 1: Index-based Pagination

When paginating over index-based endpoints that include filters (like `list_anchors`), the caller does not inherently know how many items were skipped in the underlying storage array, meaning they don't know the exact `idx` where the contract stopped scanning.
However, because the contract scans *at least* `limit` items to return `limit` matches, a client can guarantee they never miss an item by advancing `start` by `limit` in each iteration, while using a `Set` to deduplicate overlapping items. If an endpoint does not filter (like `list_assets`), no deduplication is necessary because `results.length` exactly matches the number of scanned items.

```javascript
// Example: Fully paginating through list_anchors (Index-based)
let start = 0;
const limit = 50;
const allAnchors = new Set(); // Using a Set to automatically deduplicate

while (true) {
    const page = await contract.list_anchors({ start, limit });
    
    for (const anchor of page) {
        allAnchors.add(anchor);
    }
    
    // If the contract returned fewer items than the limit, we've reached the end
    // of the underlying list.
    if (page.length < limit) break;
    
    // Advance start by `limit` to ensure we don't miss any entries.
    // Note: this may cause the next page to return some duplicates if 
    // deregistered anchors were skipped. The Set handles deduplication.
    start += limit;
}
```

### Example 2: ID-based Pagination

ID-based endpoints return structures that contain their own IDs (e.g. `Settlement` objects). To paginate, the caller simply inspects the highest ID received in the current page, and requests the next page starting at `highest_id + 1`.

```javascript
// Example: Fully paginating through list_settlements_by_asset (ID-based)
let start = 1n; // IDs are 1-based u64
const limit = 50;
const allSettlements = [];

while (true) {
    const page = await contract.list_settlements_by_asset({ asset: "USDC", start, limit });
    allSettlements.push(...page);
    
    if (page.length < limit) break;
    
    // The next page should start scanning strictly after the last found settlement
    const lastSettlement = page[page.length - 1];
    start = lastSettlement.id + 1n;
}
```
