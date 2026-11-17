# Contributing to wot-server

Use the [bug report form](https://github.com/gabrielhamel/wot-server/issues/new?template=bug.yml)
to report a problem. Include the setup, exact steps, and expected and observed
behavior. The [server guide](docs/server.md) and [testing guide](docs/testing.md)
cover local setup and test fixtures.

## Scope and design

- Give each contribution one clear purpose. Write commit messages and PR titles
  that name the affected behavior, such as `Fix batch training for newly unlocked
  crew perks` or `Add credit purchases for garage slots`.
- Identify the owning bounded context in the [context map](CONTEXT_MAP.md) and
  use its glossary in code, tests, documentation, and the PR description.
  Define shared terms where the owning context documents them.
- Put business rules in the domain, use cases in the application layer, and
  storage, transport, and presentation concerns in their respective adapters.
  Depend inward, integrate through context entry points, and keep wire types
  separate from domain models. Follow the boundaries and safe Rust requirements
  in [AGENTS.md](AGENTS.md).
- Update the relevant guides when behavior changes. Update the context READMEs,
  glossaries, CML models, and relevant source maps when context responsibilities,
  terminology, integration contracts, or file organization change. Follow the
  [testing guide](docs/testing.md) for context-map generation and verification.

## Required telemetry

Telemetry is a required part of each server change. Follow the
[telemetry contract](docs/telemetry.md) and the
[agent instructions](AGENTS.md#required-telemetry). A feature is complete when
its traces explain the request, context and layer transitions, persistence,
outcome and response.

Use stable operation names without architecture layers and set wot.context.
Omit code.layer attributes. Use available display names for user.name and current
payload emails for user.email; omit unavailable values and never query solely
to enrich telemetry. Attach explicit, bounded input attributes and useful result
counts or outcomes. Preserve
parents across async/blocking tasks, queued work and outgoing delivery. Include
complete parameterized SQL text, transaction boundaries and error status for
database operations. Keep secret credentials out of telemetry.

Review a complete trace through the affected entry point. Regression tests must
assert parent-child relationships and meaningful attributes, including relevant
failure paths. Record the trace shape, setup and results in the PR.
Instrumentation coverage and trace verification are merge requirements.

## Required manual testing

Manually verify every contribution against the proposed revision before merge.
Use the entry point affected by the change. For game-facing behavior, use the
compiled World of Tanks client against a development server running your change.
Run `python3 tools/game-client.py status --json` before client-dependent work
and record the client build and executable SHA-256. For other changes, exercise
the affected CLI, dashboard, or workflow directly. For documentation-only
changes, review the rendered pages, follow the affected links, and verify any
changed procedures.

For a fix, reproduce the reported behavior on the parent revision when possible
and repeat the same steps on the proposed revision. For a feature, verify the
complete user flow and relevant failure or recovery paths. Record the setup,
exact steps, expected outcome, and actual result in the PR. Document any
outstanding checks and complete the required manual testing before merge.

## Required regression tests

Every behavior change must add or update a regression test under the repository's
root [`tests/`](tests/) directory. Extend the existing suite for the affected
behavior and assert a result observable through the normal entry point. For a
bug fix, verify that the test fails on the parent revision for the reported
reason and passes with the fix. For a feature, cover the new behavior and its
relevant failure paths. For packet-triggered crashes, verify that the offending
input is rejected and a subsequent valid client request still succeeds.

Run the [CI checks](.github/workflows/ci.yml) locally and record the results in
the PR. Run any affected opt-in integration tests explicitly using the setup
in the [testing guide](docs/testing.md). The default suite skips tests that
require a game executable, PostgreSQL, or extracted resources. For changes
that leave behavior unchanged, explain why regression coverage is inapplicable.

## Review and merge

Link the related issue when one exists and fill out the pull request template.
Reviewers must verify the owning context and domain terminology, the recorded
manual results, and the required regression tests in `tests/` before merging.
Passing CI, manual verification and required telemetry coverage are all required.
