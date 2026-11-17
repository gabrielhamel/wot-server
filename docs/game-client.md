# Compiled game client for development

The real Windows World of Tanks client supplies `WorldOfTanks.exe`, packaged
scripts, definitions, artwork and other assets for reverse engineering and local
validation. It is separate from the decompiled `wot-src` submodule and the small
[resource bundle](resources.md) consumed by the server. Game files remain local
and retain their publisher's ownership. Ordinary server builds do not need them.

## Discover or register a client

The helper requires Python 3.11+ and uses only the standard library. From the
repository root:

```sh
python3 tools/game-client.py use '/path/to/World_of_Tanks'
python3 tools/game-client.py status
python3 tools/game-client.py status --json
python3 tools/game-client.py path
```

`use` checks for the executable and `res/packages/`, then saves the resolved
absolute directory in ignored `.local/game-client.json`. It does not copy or
modify the game. Paths with spaces and symlinks are supported. The helper itself
can be invoked from any working directory using its absolute script path.

Selection precedence is `WOT_GAME_DIR` (an environment override), the saved local
selection, then `.local/game-client/` in this checkout. An invalid explicit
selection fails with an actionable error rather than silently using another
client. Each checkout/worktree can register the same external installation.

`path` prints only the validated absolute directory. `status` also hashes the
executable and identifies `version.xml` when present. Both commands are offline
and return nonzero if the client is unavailable. Layout validation does not
verify every package, establish server compatibility or prove freshness.

## Install and update with wg-launcher

Install `wg-launcher` on `PATH` using its
[installation guide](references/wg-launcher/README.md#install-the-cli). On macOS:

```sh
brew install gabrielhamel/tap/wg-launcher
```

Then provision the client (a large download requiring substantial disk space):

```sh
python3 tools/game-client.py install
# Or choose a shared location and build selection:
python3 tools/game-client.py install --output '/path/to/World_of_Tanks' --region eu --language en
python3 tools/game-client.py check-updates
python3 tools/game-client.py update
```

Installation defaults to the selected location, or `.local/game-client/` on a
fresh checkout. The helper delegates to `wg-launcher install --game wot`, with
EU/English defaults and optional `--client-type`; an omitted variant uses the
launcher's default selection. A successful install is validated and registered.
On failure no new selection is saved; repeat the same install command and output
path to use the launcher's recovery facilities. Existing executables are protected
from `install`: register them with `use` and update explicitly instead.

`check-updates` delegates to `wg-launcher status --path`; `update` delegates to
`wg-launcher update --path`. Those commands use the installation's identity and
may access the network. For externally managed installations, follow the launcher's
diagnostics if it cannot recognize their metadata; registration alone does not
make an installation launcher-managed. The helper preserves launcher exit failures.
The launcher manages assets, not player login or running the Windows game.

Close the game before updates. Updating may replace locally modified server lists
and keys; preserve those files first as described in [client validation](client-validation.md).
Never update a historical fixture simply to satisfy a failing compatibility test.

## Agent workflow and evidence

`AGENTS.md` directs agents to run `status --json` before client-dependent research.
If missing, register a developer-provided installation or use the explicit install
command. Discovery never downloads, updates or modifies the client. Work unrelated
to the compiled client can proceed without it; report unavailable client-dependent
validation instead of treating it as passed.

Use the same location with existing tools:

```sh
export WOT_GAME_DIR="$(python3 tools/game-client.py path)"
cargo run --locked -p wot-binary-map -- "$WOT_GAME_DIR" --output artifacts/client-map
cargo run --locked -p wot-extract -- "$WOT_GAME_DIR" --output artifacts/client-resources
```

Ensure `artifacts/` exists and output directories are new where required by those
tools. The historical ignored mapper test uses `WG_GAME_FOLDER` instead:

```sh
WG_GAME_FOLDER="$WOT_GAME_DIR" cargo test -p wot-binary-map --test supplied_client -- --ignored
```

That test pins a historical executable hash and structural counts; a newer
installation is expected to differ. Do not replace its expected values without
revalidating the findings. Record the status report, actual build metadata and
relevant server/reference revision with new research. `version.xml` can describe
the displayed game version rather than the launcher's release identifier; retain
the executable hash as the precise binary identity. Historical paths and builds
in validation reports remain evidence of those past runs, not discovery defaults.

Helper checks (no real download needed):

```sh
python3 -m unittest discover -s tools -p 'test_*.py'
```
