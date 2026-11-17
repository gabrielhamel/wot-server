# Resource bundle contract

Shared filesystem contract for the offline extractor and the server's Accounts
and Client Gateway adapters. It reads a versioned manifest and resolves relative
resource keys beneath the bundle's `res/` directory. Runtime consumers
interpret the selected data in their own adapters. The optional
`game_version` records the source client's `version.xml` version for
telemetry provenance. Legacy manifests can omit it.

See the [bundle guide](../../docs/resources.md) for the data and workflow.
