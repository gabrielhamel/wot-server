# Platform

Platform is the shared technical boundary. It supplies tracing and
client-format codecs and Mercury transport without depending on runtime domain contexts. Contexts
own their protocol policy, SQL, and trace semantics. Server composition selects
and configures the telemetry exporters.

| Layer | Responsibility |
| --- | --- |
| `mercury/` | Bounded UDP framing, reliability, encryption, entity dispatch and LoginApp/BaseApp transport shared by Player Sessions and Battles. |
| `infrastructure/http.rs` | Serve HTTP/1 with a bounded connection count and connection lifetime for the shared web listener. |
| `infrastructure/observability.rs` | Trace PostgreSQL operations for their owning context and propagate spans into blocking work. |
| `infrastructure/web/` | Typed request transport, browser exception observers, Web OpenTelemetry export and trace propagation. |
| `presentation/web/` | Shared theme for the admin and Store browser views. |
| [Client formats](infrastructure/client_formats/README.md) | Handle common Packed XML, Python pickle, and binary I/O used by Accounts and Player Sessions. |

| Term | Meaning |
| --- | --- |
| Database trace adapter | Span wrapper for SQL statements and transaction boundaries with the caller's context label. |
| Codec | Reusable implementation of a file or payload format. |
