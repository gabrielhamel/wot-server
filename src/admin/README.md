# Admin

Admin owns the operator-facing HTTP API, bearer authentication and delivery of
the React Admin dashboard. [Accounts](../accounts/README.md) owns player
identities, balances, tanks and starter garage provisioning. Admin reads the
published account and catalog data and submits grants through the
[Client Gateway](../client_gateway/README.md) application API for coordination with game sessions.

| Layer | Responsibilities |
| --- | --- |
| `presentation/` | HTTP routes, operator authentication, JSON request and response models, status mapping and static dashboard routing. |
| `infrastructure/` | Bounded HTTP/1 connection serving and shutdown. |
| [`dashboard/`](../../dashboard/) | React Admin player list, creation form, per-player garage and resource grants, and token sign-in. |

| Term | Meaning |
| --- | --- |
| Admin token | Server-configured bearer credential shared with authorized operators. |
| Player record | Account ID, game login and display name returned by the admin API. |
| Grant | Operator-authorized addition of one resource or one permanently owned tank. |

The [admin guide](../../docs/admin.md) describes configuration and the HTTP
contract.

Admin HTTP traces cover method/path/status, presentation handlers, Accounts
application/repository operations and Client Gateway grant delivery. The grant queue
preserves the HTTP parent through gateway execution and response. See the
[telemetry contract](../../docs/telemetry.md).
