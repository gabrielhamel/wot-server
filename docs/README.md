# Documentation

Start with the [server guide](server.md) to run the PostgreSQL login/garage milestone.
The [WG Toolkit research reference](wg-toolkit/README.md) records earlier protocol
findings and emulator behavior. The [server guide](server.md#protocol-implementation)
maps the implementation maintained here.

The [research references](references/README.md) guide describes the four pinned
Git submodules and their setup. All `docs/references/` paths in these guides are
relative to the repository root.

The C++ BigWorld engine source is available at
`docs/references/BigWorld-Engine-14.4.1` as a reference for networking
and game code. Version 14.4.1 is an older engine version; Wargaming may have
evolved its implementation since. Findings from this source describe that
version unless confirmed against the supplied World of Tanks client or the
existing reverse-engineering work in `wg-toolkit-rs`. Its availability does not
establish compatibility with the current client.

| Topic | Guide |
| --- | --- |
| Contribution scope, manual verification, and regression tests | [Contributing](../CONTRIBUTING.md) |
| Docker startup and game server-list setup | [Docker quickstart](docker-quickstart.md) |
| Discovering, installing and updating the compiled research client | [Game client setup](game-client.md) |
| Preparing runtime game data | [Resource extraction](resources.md) |
| Publishing and running the Docker image | [Container image](container.md) |
| Required operation spans, SQL tracing, queue propagation and verification | [Telemetry](telemetry.md) |
| Complete span attribute inventory, meanings and emission conditions | [Span attributes](telemetry-attributes.md) |
| Running the server, database and client configuration | [Server guide](server.md) |
| Listing players, creating accounts and granting resources or tanks | [Admin dashboard](admin.md) |
| Creating demo players and tier I garages | [Database seeding](seeding.md) |
| Researching, buying and selling tanks, and buying garage slots | [Tank progression](vehicles.md) |
| Researching, buying and installing tank modules | [Module progression](modules.md) |
| Loading, unloading and buying tank shells | [Garage ammunition](ammunition.md) |
| Buying, mounting and demounting equipment | [Garage equipment](equipment.md) |
| Buying and mounting consumables and setting auto-resupply | [Garage consumables](consumables.md) |
| Buying and mounting equipment and crew directives | [Garage directives](directives.md) |
| Managing crew, perks, training and barracks | [Garage crew](crew.md) |
| Viewing and selling unmounted stock | [Garage Depot](depot.md) |
| Contacts, player search and private conversations | [Social protocol](social.md) |
| Safe Rust and client-packet resilience findings | [Resilience audit](resilience-audit.md) |
| Installed game configuration and live garage validation | [Client validation](client-validation.md) |
| Offline executable mapping and current findings | [Binary mapping](binary-mapping.md) |
| Implemented context relationships | [Context map](../CONTEXT_MAP.md) |
| Choosing context names from domain responsibilities and vocabulary | [Naming bounded contexts](bounded-contexts.md) |
| Mapper verification and external-client checks | [Testing](testing.md) |
| Packages, dependencies and responsibility ownership | [Workspace map](wg-toolkit/workspace.md) |
| Networking, script ABI, entities and replication | [Protocol model](wg-toolkit/protocol.md) |
| Login, hangar, account commands and battle transitions | [Session flows](wg-toolkit/session-flows.md) |
| Physics, weapons, maps and resource formats | [Gameplay and data](wg-toolkit/gameplay.md) |
| Gaps, proposed contexts and implementation milestones | [Reproduction plan](wg-toolkit/reproduction-plan.md) |
| Commands, test results and research caveats | [Verification](wg-toolkit/verification.md) |
| Every tracked Rust source file | [Source inventory](wg-toolkit/source-inventory.md) |
| IDs/methods derived from the supplied client resources | [Dispatch snapshot](wg-toolkit/dispatch-snapshot.md) |

Architecture rules and documentation maintenance requirements are in
[AGENTS.md](../AGENTS.md). The toolkit map records its audit baseline; its future
contexts remain planning material. wot-server currently implements Accounts, Social, Client Gateway, Admin plus offline Binary Mapping and Resource Extraction.
