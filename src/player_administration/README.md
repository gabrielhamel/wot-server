# Player Administration

Player Administration serves the web panel for player registration, sign-in
and a view of the signed-in player's garage with access to the Store when enabled.
Administrators share those views and get an Admin section for finding and provisioning
players, managing roles, inspecting garages, and granting currency, vehicles, cosmetics, crew books, Improved Equipment, Bounty Equipment or Experimental Equipment. [Accounts](../accounts/README.md)
owns player identities, roles, dashboard session records and garage state.
Player Administration uses the published account and catalog data and submits
grants through [Player Sessions](../player_sessions/README.md) to coordinate
with live game sessions. It authenticates dashboard Store requests and delegates
checkout to the same context.

| Layer | Responsibilities |
| --- | --- |
| `application/mod.rs` | Sign-in orchestration and session secret generation. |
| `application/admin-contracts.ts` | Browser contracts for player accounts, dashboard sessions, garages, catalogs and grants. |
| `infrastructure/admin-api.ts` | React Admin authentication and data providers, HTTP serialization and browser operation spans. |
| `presentation/` | HTTP routes, session cookies, JSON contracts and status mapping. |
| `presentation/web/` | First-administrator setup, player registration and confirmation, sign-in, player garage and Store navigation, role controls and grant forms. |

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
Accounts operations and PostgreSQL. The grant and Store queues preserve the HTTP parent
through gateway execution and response. See the [telemetry contract](../../docs/telemetry.md).

Player Administration uses the Accounts contract for player access, garage
inspection and grant catalogs. The Player Sessions contract coordinates grant
and purchase delivery. Administration supplies the embedded Store with its
dashboard HTTP transport, including the request paths and write header. Player
Administration has no database schema of its own.
