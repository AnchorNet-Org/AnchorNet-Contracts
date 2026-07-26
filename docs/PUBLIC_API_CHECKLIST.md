# Public API Compatibility Checklist

This checklist must be reviewed by contributors and maintainers before proposing or merging any changes to the public contract interface, function signatures, data structures, event payloads, or error codes in `anchornet-contracts`.

Because the contracts serve as the core state and execution layer for off-chain SDKs, mobile applications, and indexers, uncoordinated changes to the public contract API can cause severe breakage across downstream repositories.

---

## 1. API Compatibility Assessment

- [ ] **Identify the Change Type:**
  - **Non-breaking:** Adding a new optional function/entrypoint, adding a new event variant without altering existing ones.
  - **Breaking:** Modifying existing function names, changing parameter counts or types, changing return types, removing functions, altering data structure layouts (`Pool`, storage keys), changing event topic/data schema, or renumbering/removing error codes.
- [ ] **Function Signatures:**
  - Are function names, argument order, parameter types, and return types unchanged for existing public entrypoints?
  - If a function signature was altered, is there a backward-compatible alternative or migration path?
- [ ] **Data Types & Storage Layout:**
  - Are on-chain structs (e.g. in `src/types.rs`) backward compatible with existing contract state?
  - Do storage key definitions (`src/storage.rs`) preserve existing key encoding and TTL behavior?
- [ ] **Errors & Events:**
  - Are existing error code numerical values in `src/error.rs` preserved without renumbering or removing variants?
  - Are emitted event topics and data payloads (`src/events.rs`) consistent with existing off-chain indexer expectations?

---

## 2. Downstream SDK & Mobile Impact

- [ ] **SDK Bindings:**
  - Has the impact on Soroban SDK client libraries (TypeScript, Rust, etc.) been evaluated?
  - Will generated SDK client method signatures require major version bumps?
- [ ] **Mobile Integration:**
  - Will mobile applications (e.g., PocketPay Android / iOS apps) break if deployed against this contract version without a client update?
  - Is a deprecation strategy or dual-version support required for active mobile app releases?
- [ ] **Cross-Repo Coordination:**
  - Have corresponding issues or pull requests been opened in dependent repositories (SDKs, mobile apps, indexers)?
  - Are breaking changes coordinated with cross-repository release timelines?

---

## 3. Tests & Verification

- [ ] **Unit & Integration Tests:**
  - Are new unit tests added in `src/test.rs` covering modified or newly added entrypoints?
  - Do all existing unit tests pass without suppressing or removing critical assertions?
- [ ] **Snapshot & Property Tests:**
  - Have proptest regressions (`proptest-regressions/`) and snapshot tests (`test_snapshots/`) been verified or updated?
- [ ] **Local Build & Test Run:**
  - Has the contract been compiled (`cargo build`) and verified via `cargo test` and `cargo fmt --all -- --check`?

---

## 4. Documentation & Reference Updates

- [ ] **`README.md` Interface Reference:**
  - Is the Contract Interface table updated with any new or modified entrypoints, authorization requirements, and descriptions?
  - Is the Operator Permission Boundary table updated if new gated functions were introduced?
  - Is the Events list updated if new event topics or payloads are published?
- [ ] **`docs/ERRORS.md`:**
  - Is `docs/ERRORS.md` updated if error codes were added or public entrypoint mappings changed?
- [ ] **`docs/ADMIN.md` & `docs/PAGINATION.md`:**
  - Are administrative role specifications or pagination semantics updated if affected?

---

## 5. Migration Notes & Changelog

- [ ] **`CHANGELOG.md`:**
  - Is the change documented in `CHANGELOG.md` under breaking changes or new features?
- [ ] **Migration Notes:**
  - Are upgrading instructions and breaking change summaries documented for SDK developers and mobile integrators?
- [ ] **Contract Versioning:**
  - Is the `version()` entrypoint or contract metadata updated if required for this release?
