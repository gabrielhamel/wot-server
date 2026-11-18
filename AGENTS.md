# Project instructions

## Git branches

Choose descriptive Git branch names based on the work, such as
`docs/public-server-setup`. Avoid the `codex/` prefix unless explicitly
requested.

## Goal and workspace

Build a complete, functional server for World of Tanks. This repository,
`wot-server`, is the home of the server implementation and its binary.

The workspace uses the server project, four reference submodules, and a supplied
game client. Paths below are relative to the repository root unless absolute:

- `.`: the server project (repository root).
- `docs/references/wg-toolkit-rs`: the user's existing project,
  developed with a collaborator, containing parts of the World of Tanks network
  protocol and login/game client implementations. A substantial amount of
  `WorldOfTanks.exe` reverse engineering, including game structures, has already
  been done there. Consult it as a research reference. The server owns its
  protocol implementation and has no Toolkit build or runtime dependency.
- Compiled game client: run `cargo game-client status --json` before
  client-dependent work. This discovers the local executable and full resource
  tree and reports the executable SHA-256. Use
  `cargo game-client path` for its absolute directory. Selection is
  `WOT_GAME_DIR`, then ignored `.local/game-client.json`, then
  `.local/game-client/`; never assume a developer-specific absolute path.
  If unavailable, the command reports setup instructions: register an existing
  installation with `use /path/to/World_of_Tanks`, or provision one with
  `install` through `wg-launcher`. See [game client setup](docs/game-client.md).
  This is the real compiled client, including `WorldOfTanks.exe` and packaged
  resources, not the decompiled source submodule or extracted server bundle.
  Record executable identity with research findings. A directory name does not
  establish its version, freshness or server compatibility. Check for updates
  explicitly; do not update a research fixture as part of routine discovery.
- `docs/references/BigWorld-Engine-14.4.1`: the C++ BigWorld engine
  source supplied by the user as a reference for the engine Wargaming used to
  build World of Tanks. Consult it for networking and game implementation
  details. This is an older engine version; Wargaming may have changed it since.
  Treat its behavior as historical reference and verify applicability against
  the supplied game client and existing reverse-engineering findings before
  relying on it for compatibility.
- `docs/references/wot-src`: decompiled Python and ActionScript
  game code supplied by the user. Consult it for entity definitions, game
  structures, and game behavior alongside the existing protocol findings,
  supplied client executable, and BigWorld engine reference. Record source
  locations for findings and verify version applicability and decompilation
  uncertainties before treating them as confirmed compatibility requirements.
- `docs/references/wg-launcher`: reference for file organization, bounded-context
  guides, and context-map generation and automation. Adapt its architectural
  patterns to the server's responsibilities.

Initialize the reference submodules with `git submodule update --init` before
consulting them. Their revisions are pinned by this repository. Keep reference
changes separate from server implementation changes; see
[research references](docs/references/README.md) for setup and provenance.

## Living technical documentation

Keep documentation up to date throughout the work, using Markdown files under
`docs/` as needed. Document how the game server, network protocol, and relevant
game structures and behavior work, including new understanding obtained from
reverse engineering or implementation.

Update the relevant documentation as discoveries are made or behavior changes.
Distinguish confirmed findings from hypotheses and unresolved questions. Record
the supporting source, executable version, or code location when useful so that
future work can verify and extend the findings.

## Admin grants for player items

For each implemented player-usable item that players cannot buy through the
normal in-game shop, provide an authenticated grant workflow in the admin
dashboard. Use the server catalog to populate the picker and the normal account
grant transaction to update ownership and sync the garage. Preserve item
eligibility, ownership limits and in-game purchase restrictions.

## Architecture

Use `docs/references/wg-launcher` as the reference for file
organization, bounded-context guides, and the context-map system. Adapt its
patterns to the game server's actual responsibilities rather than copying its
launcher-specific contexts or product rules.

Use domain-driven design with explicit bounded contexts. Each bounded context
must contain its own layered architecture following Clean Architecture
principles, with domain concepts separated from application orchestration and
infrastructure concerns. Keep dependencies directed toward the domain and make
boundaries between contexts explicit.

Name bounded contexts after the responsibilities and vocabulary of the domain.
Use technical names for adapters and implementation details. See
[naming bounded contexts](docs/bounded-contexts.md) for examples and guidance
on supporting tools.

Put shared domain concepts with the same meaning and invariants in
`src/shared_kernel/`. Put reusable infrastructure independent of the domain in
`src/platform/`. Keep context-specific policies and published integration
contracts with the context that owns them.

At the root of each bounded context's directory, maintain a `README.md` that:

- Describes the context's purpose, responsibilities, and boundaries.
- Explains its internal layers and their roles.
- Contains a short glossary defining its ubiquitous language.

Organize production code by bounded context first, then by `domain/`,
`application/`, `infrastructure/`, and `presentation/` where needed inside each
context. Do not create crate-wide layer directories or empty layers for
symmetry. Use `tools/` when introducing workspace package boundaries; choose
packages for real responsibilities rather than making every context a crate.
Keep executable bootstrapping small, use focused modules with clear ownership,
and expose context entry points instead of reaching into another context's
internals. Keep protocol wire representations separate from domain models.
Keep the root `src/` for server contexts, their shared support, and server
composition. Give standalone tools their own workspace packages under
`tools/`. The server and tools inherit their release version from
`[workspace.package]` in the root Cargo manifest.

### Context maps

Follow the context-map system in `docs/references/wg-launcher`:

- Maintain a root `CONTEXT_MAP.md` displaying maps for production crates.
- Keep editable Context Mapper CML models in `docs/context-maps/` and rendered
  SVG diagrams in `docs/context-maps/generated/`.
- Use `cargo context-maps` to render context maps. The Rust tool and
  `.github/workflows/context-maps.yml` preserve the reference project's model
  validation, pinned tooling with checksum verification, and deterministic rendering.
- Describe actual context responsibilities, upstream/downstream relationships,
  integration contracts, and applicable DDD integration patterns. Distinguish
  implemented boundaries from planned ones; do not present a target design as
  the current implementation.
- Update CML models, context README files, glossaries, and relevant source maps
  together when boundaries, terminology, integration contracts, or file
  organization change. Follow the reference's automated SVG generation workflow.

Keep the root README focused on getting started and use `docs/README.md` as the
documentation navigation hub. Keep architectural explanations, protocol
findings, contributor guidance, and testing details in their relevant guides.

## Required telemetry

Telemetry is part of every server feature and fix. Instrumentation and trace
verification are required implementation work and merge criteria. Maintain the
[telemetry contract](docs/telemetry.md) together with the code.
Keep the [span attribute reference](docs/telemetry-attributes.md) complete when
adding, removing, renaming, or changing the meaning of span fields. Record their
units, emission conditions, and supporting source locations.

- Trace each incoming game command, XMPP stanza and admin request through its
  meaningful context and layer boundaries, database work and response or
  delivery. Use stable operation names. Set ddd.bounded_context.name on each span. Omit
  architecture layer names from span names. Instrument
  domain decisions and use cases at useful granularity.
- Emit user.name only from an available player display name and user.email only
  when the current payload supplies an email. Omit unavailable identity attributes;
  never query the database solely to enrich telemetry.
- Each operation span must carry bounded, explicit input or payload attributes
  explaining what it processed, such as account/entity IDs, command/request IDs,
  revisions, enum choices, counts and byte lengths. Record useful output counts,
  rejection reasons and delivery outcomes. Use skip_all and select fields explicitly.
- Keep one causal trace across async calls, Tokio tasks, blocking work, worker
  queues, completion channels and outgoing delivery queues. Capture the current
  span before crossing a boundary and retain it until work and response complete.
  Instrument futures rather than holding entered guards across awaits.
  Queued operation durations must include outstanding work.
- Wrap every runtime PostgreSQL command and transaction boundary with the shared
  database tracing adapter. Record complete SQL text in db.query.text, plus
  database system, operation, owning context and error status. Preserve parameter
  placeholders. Keep passwords, hashes, keys, auth tokens, connection credentials
  and raw credential envelopes out of attributes. Represent message and binary
  payloads with sizes and identifiers through an explicit allowlist.
- Mark failed operations with span error status and a diagnostic event. Record
  expected rejections, conflicts and successful no-op outcomes distinctly.
  Correlate logs with their current span. Console severity must not suppress
  operational traces sent to the collector.
- Add regression coverage inspecting exported parent IDs and attributes through
  the affected entry point, including relevant failures. Verify a complete
  request-to-response trace for changed workflows and record evidence in the PR.

## Rust safety and server resilience

The server's Rust code must be 100% safe Rust: do not introduce `unsafe` code.
Safe Rust alone does not guarantee resilience; prevent panics and uncontrolled
resource consumption in all client-facing paths as well.

Client packets, malformed or unexpected input, authentication failures,
disconnects, timeouts and other recoverable errors must never crash the server.
Reject the affected request or disconnect the affected client, log the error at
the appropriate level, and continue serving other clients. Bound client-driven
allocations, queues and work so one client cannot exhaust server resources.

Do not use `unwrap()`, `expect()`, unchecked indexing, panic-prone arithmetic or
other panic paths on client-controlled data or recoverable runtime results.
Apply this requirement across the packet-processing dependency chain.
Add regression tests for discovered packet-triggered crashes and verify that a
valid client can still be served after the offending input is rejected.

Terminate the server only for critical, unrecoverable errors that prevent safe
or correct operation, such as invalid required startup configuration, failure to
initialize required services, or an unrecoverable internal invariant violation.
Report the cause clearly and shut down gracefully where possible. Do not promote
an individual client or recoverable operation failure into a process-wide exit.

## Code clarity and comments

Prefer clear semantic names and code that communicates intent. Avoid comments
inside code when good naming and structure can explain the behavior. If a
necessary concept or decision cannot be expressed clearly through code, write
a focused comment explaining it. Keep broader explanations and reverse-
engineering knowledge in the technical documentation.
