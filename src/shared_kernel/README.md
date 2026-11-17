# Shared Kernel

Shared Kernel owns the account identifier rule used by Player Administration,
Player Sessions and Social. Accounts owns the lifecycle of player accounts
and assigns their identifiers.

| Layer | Responsibility |
| --- | --- |
| `domain/` | Defines `AccountId` and validates it independently of transport or storage. |

| Term | Meaning |
| --- | --- |
| Account ID | Positive signed 64-bit identifier for one local player account. |

Only add a concept when several contexts agree on its meaning and invariants.
Review changes with all consumers. HTTP and XMPP parse their own wire forms,
and Social owns the rule that two participants must be different accounts.
Storage and protocol types stay at their respective boundaries.
