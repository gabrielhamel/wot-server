# Container image

The [publish workflow](../.github/workflows/container.yml) builds a Linux x86-64
image and publishes it to `ghcr.io/gabrielhamel/wot-server`. It runs when a Git
tag is created and after a successful default-branch
[Latest client E2E run](../.github/workflows/latest-client-e2e.yml). The Actions
run summary shows the published image names:

- Each release publishes `<game-version>-<full-commit>` and
  `<game-version>-<git-tag>`, and updates `latest`.
  The game version comes from the extracted bundle manifest. The tag keeps the
  dotted version and any build number, removes the leading `v.` and replaces
  the separator before the build number with a hyphen.
  If the Git tag needs sanitizing or truncating, or looks like a full commit
  SHA, the image tag includes a hash of the original Git tag.

The dedicated self-hosted runner installs the latest EU production SD English
client with [wg-launcher](references/wg-launcher/README.md). Its
installed game tree persists at `/cache/wot/eu-sd-en`, and subsequent jobs
use the launcher's in-place update and download only the needed patches. The
launcher requires the complete package set to install. The full client stays on
the runner. For a tag-triggered run, the extractor and server build use the same
checkout. After a successful daily E2E run, the publisher selects the highest
numeric server release Git tag and checks GHCR for that release paired with the
tested client version. If the tag exists, the run skips publication. Otherwise it
builds that server release using the resource artifact from the E2E run. Before
building, the publisher makes the staged bundle readable by the image's
non-root user. It verifies the manifest version as that user in the final image.
Production Flux image automation follows the digest of `latest`.

The extractor and server builds share BuildKit caches for the Cargo registry
and compiled dependencies. They reuse these caches while the runner's Docker
storage is available and rebuild the workspace crates when source layers change.
The E2E test build shares the Cargo registry cache and uses a separate
cache for compiled test dependencies. The server and E2E build stages provide
Node.js and npm to Cargo. The final image packages the browser bundle produced
by the server build.

Before pushing any tags, the workflow starts the final image alongside a
fresh PostgreSQL instance on an isolated Docker network. It then copies the
resources out of that image and runs the existing E2E suite from the
release checkout against a separate test database. Both startup and the
E2E suite must pass before publication. The workflow removes the test
containers, network and local test image after the run.

## Run an image

For a complete Docker walkthrough with PostgreSQL, port mappings and game
server-list registration, follow the
[Docker quickstart](docker-quickstart.md).

The package is public and can be pulled without signing in. Set
`IMAGE` to a published image name from the workflow summary. The image bundles
the release binary with the embedded dashboard and Store, plus
`/opt/wot-server/resources`. The runtime user has UID and GID 10001.

Both the server and extractor images include the project license and
[WG Toolkit attribution](../THIRD_PARTY_NOTICES.md) in
`/usr/share/licenses/wot-server/`.

Use a PostgreSQL instance reachable from the container and export its
connection URL as `WOT_DATABASE_URL`. Mount a host directory writable by UID
10001 for the persistent login key.

```sh
docker run --rm --name wot-server \
  -p 20016:20016/udp -p 20017:20017/udp -p 20017:20017/tcp \
  -p 127.0.0.1:8080:8080/tcp \
  -e WOT_DATABASE_URL \
  --mount "type=bind,src=$PWD/state,dst=/state" \
  "$IMAGE" --resource-dir resources --private-key /state/login.pem \
  --advertised-base 192.168.1.10:20017
```

Pass the Docker host address and port reachable by game clients to
`--advertised-base`. The HTTP port mapping above publishes the dashboard on
the host's loopback interface. See the [admin guide](admin.md) for HTTPS
configuration and the [server guide](server.md) for the complete flag list.

For a local image build, generate the bundle into `resources/` using the
[extractor](resources.md), then run `docker build -t wot-server:local .`.
