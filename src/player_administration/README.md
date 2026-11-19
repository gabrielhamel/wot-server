# Player Administration

Player Administration serves the web panel for player registration, sign-in
and a read-only view of the signed-in player's garage. Administrators can
also find and provision players, manage roles, inspect garages, and grant
currency, vehicles or cosmetics. [Accounts](../accounts/README.md)
owns player identities, roles, dashboard session records and garage state.
Player Administration uses the published account and catalog data and submits
grants through [Player Sessions](../player_sessions/README.md) to coordinate
with live game sessions.

| Layer | Responsibilities |
| --- | --- |
| `application/mod.rs` | Sign-in orchestration and session secret generation. |
| `application/admin-contracts.ts` | Browser contracts for player accounts, dashboard sessions, garages, catalogs and grants. |
| `infrastructure/admin-api.ts` | React Admin authentication and data providers, HTTP serialization and browser operation spans. |
| `presentation/` | HTTP routes, session cookies, JSON contracts and status mapping. |
| `presentation/web/` | First-administrator setup, player registration and confirmation, sign-in, read-only player garage, role controls and grant forms. |

| Term | Meaning |
| --- | --- |
| Administrator | Player account whose database role authorizes player management and grants. |
| First administrator | Account created by the setup form when the database has no players. |
| Self-service registration | Player account creation enabled once an administrator exists. The email is the sign-in name and the username is the in-game display name. |
| Player record | Account ID, game login, display name and role returned by the admin API. |
| Grant | Administrator-authorized addition of currency, cosmetic copies or a permanently owned tank. |

The [admin guide](../../docs/admin.md) describes configuration and the HTTP
contract.

Player Administration continues the browser trace through HTTP handlers,
Accounts operations and PostgreSQL. The grant queue preserves the HTTP parent
through gateway execution and response. See the [telemetry contract](../../docs/telemetry.md).
