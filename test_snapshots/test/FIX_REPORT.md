Fix Report — list_settlements_anchor_status
Repo: Mitch5000/AnchorNet-Contracts
Branch: feat/anchor-status-filter · Commit: 7c8493d
Result: cargo fmt --check PASS · cargo build PASS · 250/250 tests pass

1. What the project is
A Soroban (Stellar) smart contract for AnchorNet, a cross-anchor liquidity
coordination network. Registered anchors draw on shared per-asset liquidity
pools by opening settlements, which move through a lifecycle of
Pending → Executed | Cancelled | Expired.

Settlements live in persistent storage under sequential u64 ids, so every
"list" entrypoint is an id-ordered scan from a start cursor. The codebase
had a list_settlements_by_* family filtering on one field at a time, plus one
compound anchor+asset variant.

2. The issue
An anchor asking "which of my settlements are still Pending?" had no
single-call path:

list_settlements_by_anchor → all statuses mixed together
list_settlements_by_status → every anchor's settlements
Callers had to fetch-then-filter client-side, at a cost scaling with the
anchor's entire settlement history rather than just its pending count.

3. Two pre-existing blockers found (main did not compile)
Before any feature work was possible, main was already broken. Both were
verified against a pristine clone.

#	Problem	Evidence
1	list_settlements_by_anchor_and_asset (from PR #205) is 36 chars, over Soroban's 32-char SCSYMBOL_LIMIT	error: contract function name is too long: 36, max is 32 — the crate does not build at all
2	Commit 35fd92e dropped features = ["testutils"] from the dev-dependency	322 test compile errors (Address::generate, mock_all_auths, set_sequence_number, … not found)
Commit 35fd92e's own message even acknowledges #1 as "an unrelated
pre-existing build error … being tracked separately."

Naming constraint (important)
The issue requests list_settlements_by_anchor_and_status — that is 37
characters and cannot compile. The limit comes from stellar-xdr:

Rust

pub const SCSYMBOL_LIMIT: u64 = 32;
and soroban-sdk derives the exported name directly from the Rust identifier
(derive_spec_fn.rs: let name = &format!("{}", ident)). There is no
rename/export attribute in soroban-sdk 25.3 to work around it.

Both compound filters were therefore named without the by_ infix, keeping the
codebase-wide list_* prefix:

list_settlements_anchor_status (30 chars) — new
list_settlements_anchor_asset (29 chars) — renamed, unblocks the build
4. The fix
Rust

pub fn list_settlements_anchor_status(
    env: Env,
    anchor: Address,
    status: SettlementStatus,
    start: u64,
    limit: u32,
) -> Vec<Settlement> {
    let mut out = Vec::new(&env);
    let count = storage::get_settlement_count(&env);
    let mut id = if start == 0 { 1 } else { start };
    while id <= count && (out.len() as u32) < limit {
        if let Some(settlement) = storage::get_settlement(&env, id) {
            if settlement.anchor == anchor && settlement.status == status {
                out.push_back(settlement);
            }
        }
        id += 1;
    }
    out
}
Character-for-character consistent with the existing family: same cursor
normalization (start == 0 → 1), same limit budget check on matches only
(skip-without-counting), same single-pass scan.

5. Validation
13 new tests, all passing, covering: both-field filtering across 3 anchors ×
4 statuses; full-struct integrity; equivalence against the two single-filter
intersections; skip-without-counting; cursor pagination; start == 0; unknown
anchor / registered-but-idle anchor / empty-status; lifecycle transitions;
zero-settlement contract; and the three shared pagination edge cases
(start past end, limit = 0, limit exceeding remaining).

Mutation testing — the tests actually bite
Mutant	Outcome
Drop status predicate (anchor-only)	Killed — 5 tests fail
Drop anchor predicate (status-only)	Killed — 6 tests fail
Count non-matches toward limit	Killed — 2 tests fail
Remove start == 0 normalization	Survived — provably equivalent: ids are assigned from count + 1, so id 0 never exists
Also verified the entrypoint is registered in the on-chain contract spec XDR
(not merely a Rust method) via spec_xdr_list_settlements_anchor_status().

6. Known environment limitation
make wasm (--target wasm32-unknown-unknown --release) fails with:

text

error[E0152]: duplicate lang item in crate `soroban_sdk`: `panic_impl`
This is pre-existing and unrelated to this change — proven three ways:
it reproduces on a pristine clone of main; it persists with all
dev-dependencies removed; and it is a known soroban-sdk/std interaction on
newer toolchains (here rustc 1.97.1). The native build and full test suite,
which are what CI (.github/workflows/ci.yml) actually runs, all pass.

7. Files changed
Modified
File	Change
src/lib.rs	Added list_settlements_anchor_status; renamed the 36-char asset variant to list_settlements_anchor_asset; documented the 32-char limit. (Also 3 cosmetic rustfmt rewraps required by CI — token-identical, no semantic change.)
src/test.rs	Added 13 regression tests + a 3-anchor/4-status fixture and an ids_of helper; added assert_caller_unauthorized! and fixed test_clear_operator; updated asset-variant call sites.
Cargo.toml	Restored features = ["testutils"] on the dev-dependency.
README.md	Documented the new entrypoint in the settlement table; corrected the renamed one.
docs/PAGINATION.md	Documented both compound filters and the naming constraint.
Added
29 auto-generated Soroban test snapshots under test_snapshots/test/
(the repo commits these by convention; none of the 213 existing snapshots changed).
FIX_REPORT.md — this report.