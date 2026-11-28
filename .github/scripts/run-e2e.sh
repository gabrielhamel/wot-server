#!/usr/bin/env bash
set -euo pipefail

: "${WOT_RESOURCE_DIR:?}"
test -s "$WOT_RESOURCE_DIR/manifest.json"

postgres_image=docker.io/library/postgres:18@sha256:e3d24f6906d0760bf4da0dae872415391ededefad23fd471c47dc53b0ddf3c42
database=$(docker run --detach --rm --publish 127.0.0.1::5432 \
  -e POSTGRES_DB=wot_test -e POSTGRES_USER=wot_test -e POSTGRES_PASSWORD=ci-only \
  "$postgres_image")
cleanup() {
  docker rm -fv "$database" >/dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

for attempt in {1..60}; do
  if docker exec "$database" pg_isready -q -h 127.0.0.1 -U wot_test -d wot_test; then
    break
  fi
  if [[ "$attempt" == 60 ]]; then
    docker logs "$database"
    exit 1
  fi
  sleep 1
done
address=$(docker port "$database" 5432/tcp)
if [[ ! "$address" =~ ^127\.0\.0\.1:[0-9]+$ ]]; then
  echo "::error::PostgreSQL did not publish an IPv4 loopback port."
  exit 1
fi
export WOT_TEST_DATABASE_URL="postgresql://wot_test:ci-only@$address/wot_test"

cargo +stable test --locked -p wot-battles projectile_tests -- --ignored
cargo +stable test --locked -p wot-battles rapier_scenarios::acceptance -- --ignored --test-threads=1
cargo +stable test --locked -p wot-server-runtime --lib initializes_context_storage -- --ignored
# The access lifecycle test installs a process-wide tracing subscriber.
cargo +stable test --locked --features integration-test-support --test admin_traces exports_admin_http_request_inputs -- --ignored
cargo +stable test --locked --features integration-test-support --test admin_traces exports_admin_http_access_lifecycle -- --ignored
cargo +stable test --locked -p wot-server --features integration-test-support --test garage -- --ignored
