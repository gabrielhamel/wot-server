# Telemetry contract

Telemetry is required for server behavior. Every changed workflow must be
understandable through its trace, including its context and layer transitions,
input, database work, result and response. Production sends traces to the
configured OTLP collector. See the [server guide](server.md#logging-and-telemetry)
for endpoint configuration, resource attributes and console filtering.

## Trace structure

Each game command, XMPP stanza and admin request starts an operation trace.
Spans below it represent meaningful boundaries and decisions in the owning
bounded context. The custom attribute wot.context identifies access, accounts,
social, admin, or server composition. The custom attribute code.layer identifies presentation,
application, domain, or infrastructure.

An online XMPP private message follows this hierarchy. Repository operations
can contain several query and transaction spans.

    xmpp.request                              [access / infrastructure]
      access.infrastructure.xmpp.handle       [access / infrastructure]
        social.application.open               [social / application]
          social.infrastructure.open          [social / infrastructure]
            social.infrastructure.conversations.open_conversation
              postgresql.query                [BEGIN / SELECT / INSERT / COMMIT]
        social.application.send               [social / application]
          social.infrastructure.send          [social / infrastructure]
            social.infrastructure.conversations.send_message
              postgresql.query                [BEGIN / SELECT / INSERT / COMMIT]
        access.infrastructure.enqueue         [access / infrastructure]
          xmpp.response                       [access / infrastructure]

The response may run on the recipient's connection task. It carries the
originating request's parent through the bounded output queue. Offline
delivery is stored by the original request. A later history replay belongs
to the recipient's history or presence request.

A garage mutation traces game.command, decoding/dispatch, catalog planning,
domain validation, application orchestration, repository queries, completion
handling and response. Its parent is retained across blocking catalog work,
Tokio workers and the completion queue. Queued crew commands retain their own
request parent while waiting for earlier writes. Admin grants carry the HTTP
request parent into the gateway's bounded queue.

Spans describe actual calls and work. An operation using only cached state
can have fewer children. Pure accessors and mechanical helpers do not need
spans. Layer names describe code ownership within this process. Cross-context
calls retain their own context and layer attributes.

## Attributes and outcomes

Use tracing::instrument with skip_all and an explicit field allowlist.
Include account IDs, command/request IDs, revisions, scalar command arguments,
enum choices, pagination, item counts, payload byte lengths, message
recipients and stanza IDs where relevant. Record output counts, response
codes, persistence outcomes and delivery information. Bound client-supplied
strings before attaching them. Keep player identity out of metric labels.

XMPP spans include stanza name, bounded stanza ID, input bytes, body size
and destination account ID. Mercury dispatch records command IDs, correlation
IDs, scalar arguments and array lengths. Responses include byte lengths or
business outcomes where available. HTTP traces include method, path and
response status. Business layer spans select the IDs, counts and values
relevant to their operation.

Passwords, hashes, auth tokens, private keys, database connection credentials
and complete authentication envelopes are excluded. Message content and
binary data are represented by sizes and identifiers. User identity
attributes follow the [server guide](server.md#logging-and-telemetry).

Failed operations emit diagnostic error events inside their span. The
OpenTelemetry layer uses these events to mark error status. Expected business
rejections, stale revisions and unchanged results remain distinguishable
through completion outcome attributes. A handled failure remains visible
even when writing its protocol error response succeeds.

## PostgreSQL

The shared database tracing adapter is in src/observability.rs. Application-issued queries and explicit transaction boundaries are awaited
inside postgresql.query spans.

| Attribute | Meaning |
| --- | --- |
| code.layer | infrastructure |
| wot.context | Owning context |
| db.system.name | postgresql |
| db.operation.name | SQL operation such as SELECT, UPDATE, BEGIN or COMMIT |
| db.query.text | Complete SQL command with parameter placeholders |
| error.type | Error type when execution fails |
| OpenTelemetry status | Error when execution fails |

Driver-managed migration internals and implicit rollback on a dropped
transaction are represented by the enclosing repository operation and its
error. Explicit transaction boundaries receive query spans.

Duration covers the awaited operation, including pool acquisition when
execution uses the pool. BEGIN and COMMIT receive separate spans. SQL text
is the same string passed to SQLx. Bind values stay separate from the
statement, preserving the full command without serializing credentials or
large data fields.

## Propagation and verification

Use instrument(span) or in_current_span() on async futures. Wrap blocking
closures with observability::in_current_span. Carry a cloned parent in queue
envelopes and enter it around synchronous completion handling. Entered span
guards must not cross an await. Long-lived socket and gateway loops must
not accumulate independent requests into a single trace.

The telemetry capture helper under tests/support inspects exported spans
from normal garage and XMPP flows. Regression coverage verifies same-trace
ancestry through application, domain and repository work, complete SQL text,
queued responses and database failure status. The server telemetry test
checks OTLP resource export through the server executable.

Contributions must extend these checks for changed workflows. The PR must
record the request-to-response trace shape and verification results,
including relevant errors and rejections. See the
[testing guide](testing.md) for the opt-in PostgreSQL setup.
