# Contributing to Cymesh

1. Follow the setup in [README.md](README.md).
2. Describe the affected platform and user-visible behavior in your issue or pull request. For a networking issue, describe the test topology and use test identities.
3. Run `cargo test --locked -p aegis-core` for Rust changes and `npm --prefix ui run build` for UI changes.
4. When changing visible text, update the English and Persian dictionaries and verify the LTR and RTL layouts.

Use synthetic test data and reserved example addresses in fixtures. Keep local configuration, credentials, vaults and compiled outputs outside commits. See [SECURITY.md](SECURITY.md) for private security reports.
