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

The dedicated self-hosted runner maintains the current EU production SD
English client at `/cache/wot/eu-sd-en` using
[wg-launcher](references/wg-launcher/README.md). It shares versioned bundles
and native Cargo dependencies with the daily E2E workflow. See
[E2E tests](testing.md#e2e-tests) for cache behavior.

For a tag-triggered run, the publisher uses the pushed tag. After a successful
daily E2E run, it selects the highest numeric server release tag and checks
GHCR for that release paired with the current client version. If the image
tag exists, the run skips publication. Otherwise the publisher runs native
E2E from the release checkout with fresh PostgreSQL, stages the tested
resource bundle, and builds the final image. CI scripts come from the workflow
revision so this path can test release tags created before the scripts were
introduced.

The final image packages the browser bundle produced by the server build. The
publisher verifies the game version in the packaged manifest as the image's
non-root user and checks startup against fresh PostgreSQL on an isolated
Docker network. The Actions tab can dispatch **Publish server image** to run
the release tests, build and startup check without pushing. Production Flux
image automation follows the digest of `latest`.

## Run an image

For a complete Docker walkthrough with PostgreSQL, port mappings and game
server-list registration, follow the
[Docker quickstart](docker-quickstart.md).

The package is public and can be pulled without signing in. Set
`IMAGE` to a published image name from the workflow summary. The image bundles
the release binary with the embedded dashboard and Store, plus
`/opt/wot-server/resources`. The runtime user has UID and GID 10001.

The server image includes the project license and
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
