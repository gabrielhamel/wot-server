# Shared Kernel

The `wot-shared-kernel` library publishes the shared Rust domain types.

Shared Kernel owns the account identifier rule used by Player Administration,
Player Sessions and Social. Accounts owns the lifecycle of player accounts
and assigns their identifiers. Shared Kernel also defines the equipped vehicle performance that Accounts projects, Player Sessions passes on launch, and Battles simulates.

| Layer | Responsibility |
| --- | --- |
| `domain/` | Defines `AccountId` and validates it independently of transport or storage. Defines the equipped vehicle movement, traverse and loading parameters. |

| Term | Meaning |
| --- | --- |
| Account ID | Positive signed 64-bit identifier for one local player account. |
| Vehicle performance | Base speed limits in metres per second, traverse rates in radians per second, gun arcs in radians and reload times in seconds for the equipped vehicle. |

Only add a concept when several contexts agree on its meaning and invariants.
Review changes with all consumers. HTTP and XMPP parse their own wire forms,
and Social owns the rule that two participants must be different accounts.
Storage and protocol types stay at their respective boundaries.
