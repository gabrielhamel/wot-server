# Training Rooms

Training Rooms owns hosted lobbies, joining and leaving, invitation eligibility,
team assignment and readiness. Room membership survives a round and the host's
departure closes the room. The context uses an in-memory registry and does not
persist rooms across server restarts.

Player Sessions translates native room commands and supplies the opaque member
keys and payloads used for delivery. It authenticates the live connection,
resolves equipped vehicles through Accounts, and handles handoff and return.
Battles prepares and runs the arena. The dashboard can also launch a Training
battle directly through Player Sessions.

| Layer | Responsibilities |
| --- | --- |
| `domain/` | Room settings, membership, invitation limits, roster capacity, team confirmation and readiness invalidation. |
| `application/` | Allocate room IDs, look up hosted rooms and close a room when its host leaves. |

Clients of the library use its [published contract](src/lib.rs). The
[Player Sessions adapter](../player-sessions/src/infrastructure/gateway/training.rs)
owns wire values, routing and room updates.

| Term | Meaning |
| --- | --- |
| Training Room | A discoverable lobby that holds members and settings between rounds. |
| Host | The player who creates the room and controls its settings and roster. |
| Invitation | Permission issued by a host for an account to join a room. |
| Roster slot | A team assignment or an unassigned position associated with a team. |
| Team confirmation | A host's lock on a team whose assigned members are ready and include a combat vehicle. |
