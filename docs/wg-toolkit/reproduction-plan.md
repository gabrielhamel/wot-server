# What wot-server should reproduce

[Map home](README.md) · [Session contract](session-flows.md) · [Validation](verification.md)

## Current state versus target

The original toolkit audit predates the runtime implementation. wot-server now has
Accounts and Client Gateway contexts and a PostgreSQL-backed login/garage slice alongside
offline Binary Mapping. See [current server behavior](../server.md) and the
[implemented context map](../../CONTEXT_MAP.md). The broader plan below remains
a roadmap; real-client login and garage rendering now pass for the installed `2.4.1.5514`
client; remaining lobby compatibility issues are recorded separately.

The compatibility target is the observable client-facing contract. We need
account/session behavior, spatial replication and authoritative battles; we do
not need to duplicate official server-internal process types or their private
protocols merely because BigWorld used them.

## Capability assessment

| Capability | Toolkit state | Work for a complete server |
| --- | --- | --- |
| UDP, bundles, channels, encryption | Reusable engine implementation, partial coverage | Integrate, retain regressions, investigate unsupported flag/ACK paths and resource limits |
| Script-driven ABI and typed schemas | Runtime loading/dispatch/binding implemented | Pin supported client data, detect drift, extend typed subset as needed |
| Login and handoff | Prototype challenge/admission and hosting coordination | Real identity, authentication policy, expiry/cleanup, reconnect and admission lifecycle |
| Hangar bootstrap | Account/showGUI/settings and configured vehicles | Persist account inventory, crew, currencies, preferences and coherent revisions |
| Shop and account commands | Data synchronization and a small handler subset | Implement transactional operations and explicit failures for unsupported actions |
| Queue/battle placement | Immediate per-client local battle | Shared match allocation, teams, joins/leaves and capacity handling |
| Entity/AoI/NRL replication | Significant engine-side implementation | Verify multiple human observers, owner-only state, late joins and cleanup |
| Movement/collision/destructibles | Custom physics and resource ingestion | Establish fidelity target with captured drives/maps; investigate differences |
| Guns/armour/damage | Existing reload/trajectory/hit/damage code | Correct legacy/intentional differences, extend mechanics and authority checks |
| Spotting | Distance-only enemy visibility | Camouflage, line of sight, view range, timing and shared team visibility |
| Results/progression | No complete pipeline established | Victory/time rules, results, rewards, repair/ammo economics and dossiers |
| Chat/social/external features | Ignored calls or placeholder responses | Prioritize needed features; implement services or compatible disabled/error states |
| Operations | CLI, traces, local/TCP links | Configuration, graceful shutdown, observability, persistence recovery and capacity controls |

## Proposed bounded contexts

Organize by responsibility first. These are candidate boundaries to refine through
implementation, not a mandate to create one crate per row or empty directories.

| Candidate context | Domain responsibilities | Inbound/outbound contracts |
| --- | --- | --- |
| Client Gateway | Admission, authenticated identity, session state and handoff | Login wire adapter; account identity lookup; battle destination/session transfer |
| Accounts | Owned vehicles/crew/items, balances, settings and account revisions | Sync/query/update use cases; durable repository; battle loadout snapshot and results application |
| Catalog | Supported client version, vehicle/module/map definitions and rules data | Validated catalog snapshots consumed by Accounts and Battles; resource loading adapters |
| Battle Allocation | Queue membership, match formation, teams and battle placement | Account/loadout eligibility; session destination; battle creation request |
| Battles | Match lifecycle, vehicle state, controls, physics, weapons, spotting and outcomes | Immutable catalog/loadout inputs; authorized player commands; replication projections; final results |

Engine protocol integration is initially infrastructure supporting those contexts,
not automatically a business bounded context. Keep reusable transport/entity
machinery separate from WoT domain concepts. Observation/capture tools can remain
development infrastructure until a package boundary is justified.

Suggested dependency direction: Catalog publishes versioned definitions to
Accounts and Battles; Accounts supplies immutable eligible loadouts to Battle
Allocation/Battles; Allocation requests battle creation and returns destinations
to Sessions; Battles publishes results that Accounts applies once. Consumers
translate external data through adapters (an anti-corruption layer around
`wgtk` values/schemas). Define these as explicit contracts rather than sharing
mutable toolkit account/world structures between contexts.

Within each implemented context, keep domain rules independent, application use
cases coordinating them, infrastructure adapting resources/storage/networking,
and presentation exposing entry points where needed. Create its README with
purpose, boundary, layer ownership and glossary at the same time as its code.

When actual production contexts are introduced, follow
[AGENTS.md](../../AGENTS.md): maintain root `CONTEXT_MAP.md`, CML models under
`docs/context-maps/`, deterministic generated SVGs, and the pinned/checksummed
renderer/workflow adapted from the
[wg-launcher submodule](../references/wg-launcher/README.md). This audit changes
documentation only and does not claim that proposed contexts are implemented or generate a
production context map for the current stub.

## Reproduction sequence and acceptance evidence

| Stage | Build on | Observable completion criterion |
| --- | --- | --- |
| 1. Version and engine foundation | `res`, `script`, `dispatch`, `entity`, `net` | Supplied resource model loads, required schemas bind, packet/element regressions pass; incompatible definitions fail clearly |
| 2. Session to hangar | Login/base lifecycle, account/shop payloads | Real client connects, enables Account, loads configured vehicle/crew, synchronizes shop/account and returns coherent errors for unsupported actions |
| 3. Reference bot battle | Existing handoff, Battle, World, gun/armour | Repeatable enter/ready/move/aim/fire/damage/destruction/leave scenario with correct resource IDs and cleanup |
| 4. Shared human battle | Spaces, witnesses, authority and controllers | Two real clients share one world, see each other's motion/health, obey ownership/visibility and disconnect safely |
| 5. Durable progression | Account persistence and battle outcomes | Restart preserves inventory/balances; battle results are applied once; reload/ammo/reward changes synchronize coherently |
| 6. Fidelity and completeness | Captures, drive comparison, targeted research | Defined compatibility matrix covers maps/vehicles/mechanics; each known approximation is fixed or explicitly scoped |

Stage 3 is a behavioral baseline, not permission to carry all temporary account
or isolated-battle assumptions into the target design. Retain a runnable vertical
slice while moving responsibilities into their owning contexts.

## Questions to resolve with evidence

- Which account commands are required for the intended first playable feature set, beyond the currently handled subset?
- Does base-relayed cell traffic remain sufficient for every supported client scenario? Implement a direct CellApp listener only if evidence requires it.
- What is the actual meaning of uncertain creation fields such as `CreateCellPlayer.vehicle_id`? The emulator fills it with its own vehicle entity ID; historical captures questioned that interpretation. Preserve bytes and investigate before naming a domain concept after it.
- Which component-ordering rules generalize beyond the documented captures and this resource build?
- Which modern weapon, crew, equipment and game-mode behaviors need implementing beyond the stock/default loaders?
- Which end-of-match rules and account economics should define the first complete playable mode?

Use [verification tooling](verification.md) to answer each question with a named
build, source location, capture/scenario, observed result and remaining uncertainty.
