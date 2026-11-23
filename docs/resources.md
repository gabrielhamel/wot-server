# Server resource extraction

Run the extractor where World of Tanks is installed. Use the game build your
players will connect with and build the extractor from the same source revision
as the server.

```sh
cargo build --locked --release -p wot-extract
./target/release/wot-extract '/path/to/World_of_Tanks' --output /path/to/resources
```

The output directory must be new and its parent must already exist. After
successful extraction, copy the entire output directory to the server host.
The default location is `resources/` beside the server executable. To use
another location, set `resource_dir` in `server.json`. Replace the old
`game_dir` field when upgrading an existing configuration.

The [published container image](container.md) includes the bundle. To use a
different bundle, mount it read-only and set `resource_dir` to its absolute
path inside the container. The extractor runs offline with the installed client
as input. The server reads the bundle on startup and on demand for vehicle
templates, so keep its files available for the lifetime of the process.

## Bundle contract

The version 1 bundle contains `manifest.json` and a `res/` tree of selected
definitions. The manifest records which component client classes exist so
Player Sessions can reconstruct the network replication component IDs. The extractor
reads the original package file indexes for class presence and copies zero
Python bytecode. When the installation has `version.xml`, the extractor
also records its trimmed `version` text in the optional `game_version`
manifest field. This travels with the bundle when it is moved to another
server. Older manifests without the field remain supported.

- **Player Sessions** uses Packed XML aliases, entity and component lists and definitions,
  interface definitions and extension descriptors for the protocol ABI. Training Room
  creation validates the selected arena against `scripts/arena_defs/`.
  **Start Battle** also needs the map's outdoor height tiles. The extractor
  keeps `space.settings` and `terrain2/heights` under the bundled `spaces/`
  tree. Re-extract a bundle without terrain before launching training battles.
  Bundles without arena definitions still serve the garage.
- **Battles** uses `terrain2/dominanttextures` for driving surfaces. The extractor
  stores these as `.mat` files beside the corresponding `.hmp` height tiles.
  Re-extract existing bundles to enable hard, medium and soft terrain. Heights-only
  bundles use medium ground everywhere.
- **Battles** reads the generated `spaces/<map>/collision.bin` placement
  files and shared `collision-models/*.mesh` triangle data for projectile
  impacts. The extractor derives these from the compiled space and its
  referenced Havok meshes. It omits the original assets and reports the
  number of missing or unsupported collision models. Each placement stores
  a mesh reference, visibility mask and affine transform. Each mesh stores
  triangles and their material IDs. Battles uses
  `system/data/material_kinds.xml` to resolve the client effect index.
  **Accounts** reads the legacy and prefab shot effect tables and the common
  vehicle defaults. Re-extract existing bundles to enable obstacle impacts
  and select the loaded shell's effect definitions.
- **Accounts** uses vehicle, module and shell definitions, crew data, garage
  item definitions, customization price groups, style, camouflage, paint, decal, projected decal, effect, personal-number attachment and stat tracker definitions from the client, plus
  the personal-number restriction list and regular tech trees for starter garages and available vehicle
  name catalogs for admin labels. The optional `vehicle_customization.mo` supplies localized attachment and stat tracker names. Older bundles fall back to the attachment definition name, with “Unnamed attachment” for entries without a name.
  The Store also uses `artefacts.mo`, `crew_skins.mo` and
  `gb_vehicles.mo` for localized service items, skins and British vehicles.

The extractor copies selected definition files as complete files and preserves
their resource paths. It omits executables, packages, artwork, map assets and
audio. Both bundles and game files should stay outside Git.

Package files take precedence over loose files. The extractor uses package
order from `paths.xml` where present and considers remaining installed packages
in filename order. Mod overlays are outside the extraction scope. Resource
requirements come from the current [garage catalog](../src/accounts/src/infrastructure/client)
and [script ABI loader](../src/shared-infrastructure/src/mercury/script/load.rs).
Regenerate the bundle when changing the game build or updating to a server
that reads different resource paths. For [garage camouflage](camouflage.md),
regenerate existing bundles with the updated extractor. Each cosmetic category
requires its extracted definition list for purchases. The format version identifies the bundle
contract and the extracted Account definition supplies the protocol's required
client version.
