# Player Administration

Player Administration gives authorized operators the ability to find and
provision players, inspect garages, and grant currency, vehicles or cosmetics. The HTTP
API and dashboard expose those workflows. [Accounts](../accounts/README.md)
owns player identities and garage state. Player Administration uses the
published account and catalog data and submits grants through
[Player Sessions](../player_sessions/README.md) to coordinate with live sessions.

| Layer | Responsibilities |
| --- | --- |
| `presentation/` | HTTP routes, operator authentication, JSON request and response models, and status mapping. |
| [Platform HTTP](../platform/infrastructure/http.rs) | Shared bounded HTTP/1 connection serving and shutdown. |
| `application/admin-contracts.ts` | Typed contracts for player accounts, garages, catalogs and grants. |
| `infrastructure/admin-api.ts` | React Admin authentication and data providers, HTTP serialization and browser operation spans. |
| `presentation/web/` | Token sign-in, player list and creation, and the garage page and grant forms. |

| Term | Meaning |
| --- | --- |
| Admin token | Server-configured bearer credential shared with authorized operators. |
| Player record | Account ID, game login and display name returned by the admin API. |
| Grant | Operator-authorized addition of currency, cosmetic copies or a permanently owned tank. |

The [admin guide](../../docs/admin.md) describes configuration and the HTTP
contract.

Player Administration HTTP traces cover method, route, status, allowlisted
request inputs, validated caller and immediate peer IPs, presentation handlers, Accounts operations, and Player
Sessions grant delivery. The admin API continues the browser trace, and the grant queue preserves the HTTP parent through
gateway execution and response. See the
[telemetry contract](../../docs/telemetry.md).
