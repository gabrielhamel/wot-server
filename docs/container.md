# Container image

The [publish workflow](../.github/workflows/container.yml) builds a Linux x86-64
image when a Git tag is created and publishes it to
`ghcr.io/gabrielhamel/wot-server`. The Actions run summary shows the published
image names:

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
launcher requires the complete package set to install. The extractor and
server build come from the same checkout. The full client stays on the runner.
The workflow logs the extracted version and verifies that the final image
contains the same manifest version before publishing.

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
`/opt/wot-server/resources`. Existing UI overrides can still use `web/` or
`dashboard/` beside the binary. The server creates `server.json` there on first start. The runtime user has UID and GID 10001.

Both the server and extractor images include the project license and
[WG Toolkit attribution](../THIRD_PARTY_NOTICES.md) in
`/usr/share/licenses/wot-server/`.

Use a PostgreSQL instance reachable from the container, and prepare a
`server.json` with its connection string. Keep the configuration and login
identity on the host. For example, with a pre-existing `./state` directory
writable by UID 10001 and a readable `./server.json`:

```sh
docker run --rm --name wot-server \
  -p 20016:20016/udp -p 20017:20017/udp -p 20017:20017/tcp \
  -p 127.0.0.1:8080:8080/tcp \
  --mount "type=bind,src=$PWD/server.json,dst=/opt/wot-server/server.json,readonly" \
  --mount "type=bind,src=$PWD/state,dst=/state" \
  "$IMAGE"
```

Set `private_key` to `/state/login.pem` and `advertised_base` to the host address and port that
game clients can reach; the container's bridge address is usually unreachable
from outside Docker. The dashboard listens on all container interfaces by default.
The port mapping above publishes it on the host's loopback interface. See the
[admin guide](admin.md) for HTTPS configuration and the [server guide](server.md)
for database, RSA login and client setup.

For a local image build, generate the bundle into `resources/` using the
[extractor](resources.md), then run `docker build -t wot-server:local .`.
