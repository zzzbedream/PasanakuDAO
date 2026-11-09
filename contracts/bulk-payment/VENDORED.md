# Vendored: Trezu bulk-payment contract

Source: https://github.com/NEAR-DevHub/trezu/tree/9c1ca530220666147b4d7ab8d365cd53c9395bb7/contracts/bulk-payment
Commit: 9c1ca530220666147b4d7ab8d365cd53c9395bb7
License: MIT (see repository)

Files copied unchanged: `src/lib.rs`, `README.md`.
`Cargo.toml` is adapted only to join this workspace (shared `[profile.release]`,
dev-dependencies limited to `near-sdk` unit-testing). The upstream sandbox
integration tests (`tests/integration_tests.rs`) are not vendored; this repo
exercises the contract from `contracts/integration-tests` instead.
