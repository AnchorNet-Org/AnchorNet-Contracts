# Clippy audit

This PR adds a failing-on-warning Clippy gate for the production contract
library. The command is exposed through `make clippy` and is the exact command
run by CI:

```bash
cargo clippy --lib -- -D warnings
```

## Scope decision

The first run was performed against the pinned `stable` toolchain from
`rust-toolchain.toml` before lint fixes. Production code produced 38 findings:

| Category | Count | Resolution |
| --- | ---: | --- |
| `deprecated` (`Events::publish`) | 27 | Deferred; the established event wire format is compatibility-sensitive |
| `clippy::unnecessary_cast` | 10 | Fixed; `soroban_sdk::Vec::len()` is already `u32` |
| `dead_code` (`has_pending_admin`) | 1 | Retained as a typed storage probe with a narrow allow and rationale |

The production scope now exits successfully with `-D warnings`.

Test targets were intentionally deferred. `cargo clippy --all-targets
-- -D warnings` cannot reach a lint inventory because the checked-in event
regression tests do not compile against the pinned Soroban SDK 25.3 API: they
use the removed `Address::random` and `ContractEvents::len/get` interfaces.
The same compatibility errors prevent `cargo test` from compiling the full
suite. Updating those tests is a separate compatibility migration, not a
Clippy suppression or a behavioral fix for this PR.

## Allow policy

- `src/events.rs` has one module-scoped `#[allow(deprecated)]`. It is limited
  to the legacy event publishing helpers. Replacing `Events::publish` with
  `#[contractevent]` can change the serialized event shape, so that migration
  is deliberately deferred until the compatibility contract is reviewed.
- `src/storage.rs` keeps narrow `#[allow(dead_code)]` annotations for
  `has_pending_admin` and `has_min_liquidity`. Both are typed storage probes
  retained for the accessor surface and documented as currently unused; they
  are not blanket suppressions.
- No crate-level blanket allow was added.

## Generated code and behavior

The ten cast removals are type-preserving (`Vec::len()` already returns
`u32`) and do not alter contract behavior. No wasm byte delta is available
from this checkout because the `wasm32-unknown-unknown` target is not
installed; the production source changes are lint-only and the event helpers
are unchanged.

