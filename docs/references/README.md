# Research references

These Git submodules contain upstream research and architecture references.
All paths in this table are relative to the wot-server repository root.

| Path | Upstream | Tracking branch | Purpose |
| --- | --- | --- | --- |
| `docs/references/wg-toolkit-rs` | [theorzr/wg-toolkit-rs](https://github.com/theorzr/wg-toolkit-rs) | `main` | Protocol, client reverse engineering, and emulator behavior |
| `docs/references/BigWorld-Engine-14.4.1` | [v2v3v4/BigWorld-Engine-14.4.1](https://github.com/v2v3v4/BigWorld-Engine-14.4.1) | `main` | Historical C++ engine implementation |
| `docs/references/wot-src` | [izeberg/wot-src](https://github.com/izeberg/wot-src) | `EU` | Decompiled Python and ActionScript client code |
| `docs/references/wg-launcher` | [gabrielhamel/wg-launcher](https://github.com/gabrielhamel/wg-launcher) | `main` | File organization, bounded-context guides and context-map tooling |

From an existing wot-server checkout, initialize the pinned revisions:

```sh
git submodule update --init
```

Alternatively, use `git clone --recurse-submodules <wot-server-repository-url>`
for a new checkout. Server builds and runtime do not require these submodules.
The compiled game client is a separate local resource, discovered and provisioned
with the [game client helper](../game-client.md); its files are never committed.

Git records each exact reference revision in the parent repository. Inspect the
pins with `git submodule status`. The tracking branches above are configured in
`.gitmodules`; ordinary initialization restores the recorded commits. To advance
to the configured branch tips, run from the repository root:

```sh
git submodule update --remote
```

The [Toolkit research map](../wg-toolkit/README.md) retains its documented
2026-11-11 audit baseline, `357a7f556a197079c21a371be5718e5211b41eda`. Citations
to paths removed from the current Toolkit use that historical GitHub revision.

Treat reference revisions as evidence, not proof of compatibility. BigWorld
14.4.1 is historical; decompiled sources may target a different game build and
contain decompilation errors. Check the pinned source's version metadata and the
supplied executable before applying findings. Existing documentation retains the
version and provenance of its original observations; adding these submodules does
not revalidate those observations against a new client.

When intentionally updating a reference, advance it with the command above or
check out a desired upstream commit, review affected findings and links, then stage its path in
the parent repository to record the new pin. Keep upstream source changes separate
from server implementation work and update the relevant research documentation.
