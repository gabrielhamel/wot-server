# Context map generator

Run `cargo context-maps` from the workspace with Java 17 or newer and Graphviz
installed. Set `JAVA_HOME` or put `java` on `PATH`, and put `dot` on `PATH`.

The tool downloads the pinned Context Mapper CLI from Maven Central, verifies
its SHA-256, validates every CML model, and renders and checks SVGs in a temporary
directory. It normalizes the randomized Graphviz attributes and publishes only
after all models succeed. Use `--models-dir` to render models from a different
directory.

See the [context map](../../CONTEXT_MAP.md) and
[automated workflow](../../.github/workflows/context-maps.yml).
