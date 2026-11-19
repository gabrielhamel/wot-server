# Bounded contexts

## Naming and ubiquitous language

A bounded context defines where a model and its vocabulary have a consistent
meaning. Start by identifying the decisions the context owns and the terms used
by the people who work with those decisions. Choose a name that makes those
responsibilities recognizable to a player, operator, or domain specialist.

- Name the responsibility. **Battle Allocation** describes deciding which
  players enter which battle. A queue or worker is one way to carry out that decision.
- Check that the name survives an implementation change. **Social** still
  describes contacts and private conversations if the chat protocol changes.
  Protocols, transports, databases, UI frameworks, and architecture patterns
  belong in the names of adapters or in the context's implementation guide.
- Write one sentence describing what the context owns. Check that the name,
  glossary, and public contracts describe the same responsibility. If that
  sentence needs unrelated responsibilities, review the boundary before renaming.
- Distinguish player and operator contexts from supporting tools. A technical
  discipline can have its own vocabulary and model. Use that vocabulary for a
  tool's scope and label its role in the map.

## Where shared code goes

Put a domain value in [Shared Kernel](../src/shared_kernel/README.md) when
multiple contexts need the same meaning and invariants. Keep the model small
and review changes with its consumers. A context's published contract stays
with its owner. The map shows the extracted kernel and its consumers using
[upstream and downstream relationships](https://contextmapper.org/docs/ar-extract-shared-kernel/).

Put reusable infrastructure in [Platform](../src/platform/README.md) when it
works independently of a domain context. Infrastructure specific to a workflow
stays with that workflow. Offline tools with their own inputs, outputs, and
research vocabulary remain explicit boundaries.

## Published contracts

Publish a contract at the owning context's entry point for every interaction
with another context. Document what each operation accepts, what it returns,
and who owns its invariants. The owner runs the command and commits its changes.
Consumers receive a result or read projection and adapt it to their own
language. Server composition creates the adapters and injects the services.

The published contract is the only dependency a consumer may take on another
context. Internal modules, helpers, repositories, database handles, and
persistence models are private to their owner. Rust visibility such as
`pub(crate)` allows access within the binary. The context's entry point defines
which operations and types it publishes across the boundary. This lets the
owner change its implementation independently and keeps its invariants in one
place.

For example, Social resolves numeric player IDs through Accounts'
[`PlayerDirectory`](../src/accounts/contract.rs). Player Sessions submits
revision-checked commands through [Accounts' garage API](../src/accounts/application/garage.rs)
and delivers the committed result to the connected client. These calls cross
the same boundaries inside one binary that they would cross after extraction.

## PostgreSQL ownership and migrations

The server shares one PostgreSQL database and connection pool. Each persistent
context owns one schema and its own SQLx migration history. Define the context's
starting schema in one initial migration under its `infrastructure/migrations/`
directory. Add subsequent schema changes as new migrations. Keep runtime queries
schema-qualified. Contexts that hold state in memory need no schema.

Keep foreign keys, joins, views, and triggers within one context. Persist a
reference to another context as a scalar ID. Resolve or validate it through
the owner's published contract, and account for missing identities in the
consuming context. Calls that need another context's database access run
outside the caller's database transaction so both can use a single connection.

Accounts and Social track migrations in their own schemas. Applied context
migrations retain their checksums. See the [server guide](server.md#running)
for database setup.

## Review of the current names

The [context map](../CONTEXT_MAP.md) uses the names found in the code today.

| Current name | Assessment |
| --- | --- |
| Accounts | Familiar domain language for identity and account ownership. The context also owns much of the garage and progression. Revisit that boundary if those rules grow into a separate model. |
| Social | Fits contacts, presence as seen by other players, and private conversations. |
| Player Sessions | Names the lifecycle it owns, from admission and handoff to disconnection. Mercury, XMPP, and garage command translation are responsibilities of its adapters. |
| Player Administration | Names the operator workflow for provisioning players, inspecting garages, and granting resources or vehicles. |
| Shared Kernel | Names the small domain model maintained for use across contexts. It currently defines the local account ID. |
| Platform | Names the shared technical services used by the runtime contexts. It currently provides codecs and trace adapters. |
| Resource Extraction | Clear name for an offline compatibility tool that publishes a resource bundle for the runtime. Its language comes from the tool's users. |
| Binary Mapping | Clear name for the offline research workflow that maps executable structures and preserves the evidence for those mappings. |
| Game Client | The external system on the map. Its name identifies the integration partner. |
