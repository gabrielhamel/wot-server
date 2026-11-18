# Player Administration

Player Administration gives authorized operators the ability to find and
provision players, inspect garages, and grant currency, vehicles or cosmetics. The HTTP
API and dashboard expose those workflows. [Accounts](../accounts/README.md)
owns player identities and garage state. Player Administration uses the
published account and catalog data and submits grants through
[Player Sessions](../player_sessions/README.md) to coordinate with live sessions.

| Layer | Responsibilities |
| --- | --- |
| `presentation/` | HTTP routes, operator authentication, JSON request and response models, status mapping and static dashboard routing. |
| [Platform HTTP](../platform/infrastructure/http.rs) | Shared bounded HTTP/1 connection serving and shutdown. |
| [`dashboard/`](../../dashboard/) | React Admin player list, creation form, per-player garage and currency, tank and 3D attachment and stat tracker grants, and token sign-in. |

| Term | Meaning |
| --- | --- |
| Admin token | Server-configured bearer credential shared with authorized operators. |
| Player record | Account ID, game login and display name returned by the admin API. |
| Grant | Operator-authorized addition of currency, cosmetic copies or a permanently owned tank. |

The [admin guide](../../docs/admin.md) describes configuration and the HTTP
contract.

Player Administration HTTP traces cover method, route, status, allowlisted
request inputs, validated caller and immediate peer IPs, presentation handlers, Accounts operations, and Player
Sessions grant delivery. The grant queue preserves the HTTP parent through
gateway execution and response. See the
[telemetry contract](../../docs/telemetry.md).
