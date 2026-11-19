# Context map generator

Run `cargo context-maps` from the workspace with Java 17 or newer and Graphviz
installed. Set `JAVA_HOME` or put `java` on `PATH`, and put `dot` on `PATH`.

The tool downloads the pinned Context Mapper CLI from Maven Central and tries
the Google-hosted Maven Central mirror if the download fails. It verifies the
archive's SHA-256, validates every CML model, and renders and checks SVGs in a
temporary directory. It normalizes the randomized Graphviz attributes and
publishes only after all models succeed. Use `--models-dir` to render models
from a different directory.

See the [context map](../../CONTEXT_MAP.md) and
[automated workflow](../../.github/workflows/context-maps.yml).

The publishing job uses a private GitHub App installed on this repository with
**Contents: Read and write**. Add the app to the default branch ruleset's
**Always allow** bypass list. Set the repository variable
`CONTEXT_MAPS_APP_CLIENT_ID` and Actions secret `CONTEXT_MAPS_APP_PRIVATE_KEY`
from that app. The render job uses the standard read-only workflow token.
