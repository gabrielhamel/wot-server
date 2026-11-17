# Standalone seeder

This package provides the `seed` command used to provision demo players and
tier I garages. Run `cargo seed` from the workspace, or run
`cargo run -p wot-seed -- --help` for configuration options.

The CLI reads database and resource settings, calls the Accounts provisioning
operation, and reports the result. Accounts owns the provisioning rules and
database writes. The command runs without starting the server listeners.
See the [seeding guide](../../docs/seeding.md).
