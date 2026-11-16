# WG Toolkit research reference

[Documentation hub](../README.md) · [Source inventory](source-inventory.md) · [Reproduction plan](reproduction-plan.md)

## Scope and evidence

The toolkit submodule at `docs/references/wg-toolkit-rs` (repository-relative)
is a research reference. The [server implementation](../server.md#protocol-implementation)
lives in `wot-server`.

Mapped on **2026-11-11** from the clean `wg-toolkit-rs` checkout at
`357a7f556a197079c21a371be5718e5211b41eda`. Workspace packages declare version
`0.5.0`, Rust edition 2024. This is a source and local validation map, not a claim
that every feature works end to end with the Windows client.

The audit used a supplied resource directory named for EU `2.4.1.5498`;
the directory name did not independently establish the build. Discover the current
local resource reference through [game client setup](../game-client.md).
This audit did not independently extract the executable version, hash it, or
disassemble it. Findings inherited
from prior reverse engineering are identified as such.

**The project already includes a substantial server emulator.** Reproducing only
its network codecs would miss account synchronization, session transitions,
entity replication, resource loading, physics, weapons, and client presentation
contracts. Conversely, its working implementation is not a complete authoritative
multiplayer service: accounts are temporary, each client's battle is isolated,
and important gameplay systems are simplified or absent.

## Reading routes

| Question | Guide |
| --- | --- |
| What lives where, and which pieces are reusable? | [Workspace and dependency map](workspace.md) |
| How do UDP, login, entities, components, and replication fit together? | [Protocol and entity model](protocol.md) |
| What must happen between login, hangar, battle, and return? | [Session and account flows](session-flows.md) |
| What simulation and resource handling already exist? | [Gameplay and game data](gameplay.md) |
| What should wot-server reproduce first, and what remains missing? | [Reproduction plan and proposed boundaries](reproduction-plan.md) |
| How do we inspect and validate the implementation? | [Tools, evidence, and verification](verification.md) |
| Where is a particular source file? | [Complete tracked Rust source inventory](source-inventory.md) |
| What IDs does this resource set actually produce? | [Resource-derived dispatch snapshot](dispatch-snapshot.md) |

Suggested first pass: workspace → session flows → reproduction plan. Use the
protocol and gameplay guides while implementing a particular capability.

## Evidence vocabulary

- **Implemented:** a concrete code path exists in the inspected revision. This
  alone does not prove official-server parity or a successful real-client run.
- **Locally checked:** a command executed during this audit; its scope and results
  are in [verification](verification.md#checks-performed).
- **Reported finding:** a source comment or existing research note reports a
  capture, client-script, SDK, or disassembly result. Its original artifacts have
  not necessarily been supplied here or revalidated.
- **Proposed:** an implementation order or boundary for wot-server, not code that
  already exists in this repository.

## Historical documentation

Read the toolkit's [overview](../references/wg-toolkit-rs/README.md),
[apps](../references/wg-toolkit-rs/doc/APPS.md),
[entities](../references/wg-toolkit-rs/doc/ENTITY.md),
[hangar research](../references/wg-toolkit-rs/doc/HANGAR_LOADING.md), and
[proxy lessons](../references/wg-toolkit-rs/doc/PROXY.md) alongside the source.
They contain useful reasoning and historical evidence, but some paths and gaps
are obsolete. In particular, `wg-toolkit-cli/src/bootstrap/` and generated
`src/wot/gen/` code are no longer present: runtime script loading, dynamic
dispatch, and typed schema binding replaced them. Cell simulation now exists
inside the base app, despite older descriptions saying only cell IDs exist.
A standalone direct CellApp socket server still does not exist.

The linked `re-work/...` and Frida scripts mentioned in older notes are not
present in the toolkit checkout. Decompiled client code and the BigWorld engine
are available as separate [research submodules](../references/README.md); verify
that their versions match any historical claim before relying on them.

## Maintaining this map

When updating the source baseline, refresh the commit, inspect changes in each
responsibility, regenerate resource-derived tables, and rerun the checks. Keep
uncertainties distinct from established wire layouts. Source links point into
the research submodule at the revision pinned by wot-server. Paths removed from
the tracked `main` branch link to the audited GitHub revision instead. The audit
baseline above remains the provenance for these findings; updating a submodule
does not by itself revalidate the map.
