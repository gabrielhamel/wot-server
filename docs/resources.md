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
`game_dir` field when upgrading an existing configuration. The
[demo seeder](seeding.md) uses the same bundle.

The [published container image](container.md) includes the bundle. To use a
different bundle, mount it read-only and set `resource_dir` to its absolute
path inside the container. The extractor runs offline with the installed client
as input. The server reads the bundle on startup and on demand for vehicle
templates, so keep its files available for the lifetime of the process.

## Bundle contract

The version 1 bundle contains `manifest.json` and a `res/` tree of selected
definitions. The manifest records which component client classes exist so
Access can reconstruct the network replication component IDs. The extractor
reads the original package file indexes for class presence and copies zero
Python bytecode.

- **Access** uses Packed XML aliases, entity and component lists and definitions,
  interface definitions and extension descriptors for the protocol ABI.
- **Accounts** uses vehicle, module and shell definitions, crew data, garage
  item definitions, regular tech trees for demo seeding and available vehicle
  name catalogs for admin labels.

The extractor copies selected definition files as complete files and preserves
their resource paths. It omits executables, packages, artwork, map assets and
audio. With installed EU `2.4.1.5514` the selected files total about 83 MB
before compression. Both bundles and game files should stay outside Git.

Package files take precedence over loose files. The extractor uses package
order from `paths.xml` where present and considers remaining installed packages
in filename order. Mod overlays are outside the extraction scope. Resource
requirements come from the current [garage catalog](../src/accounts/infrastructure/client)
and [script ABI loader](../src/access/infrastructure/mercury/script/load.rs).
Regenerate the bundle when changing the game build or updating to a server
that reads different resource paths. The format version identifies the bundle
contract and the extracted Account definition supplies the protocol's required
client version.
