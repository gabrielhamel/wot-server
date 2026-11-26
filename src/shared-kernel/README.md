# Shared Kernel

The `wot-shared-kernel` library publishes the shared Rust domain types.

Shared Kernel owns the account identifier rule used by Player Administration,
Player Sessions and Social. Accounts owns the lifecycle of player accounts
and assigns their identifiers. Shared Kernel also defines the equipped vehicle
performance and seasonal outfits that Accounts projects and Player Sessions
passes to Battles at launch.

| Layer | Responsibility |
| --- | --- |
| `domain/` | Defines `AccountId` and validates it independently of transport or storage. Defines the equipped vehicle performance and encoded outfits by arena season. |

| Term | Meaning |
| --- | --- |
| Account ID | Positive signed 64-bit identifier for one local player account. |
| Battle outfits | Encoded appearances for an equipped vehicle for winter, summer and desert arenas. |
| Vehicle performance | Base speed limits in metres per second, traverse rates in radians per second, gun arcs in radians, reload times in seconds, armor meshes, optional engine compartment anchors and module health for the equipped vehicle. |

Only add a concept when several contexts agree on its meaning and invariants.
Review changes with all consumers. HTTP and XMPP parse their own wire forms,
and Social owns the rule that two participants must be different accounts.
Storage and protocol types stay at their respective boundaries.
