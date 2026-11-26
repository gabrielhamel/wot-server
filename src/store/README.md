# Store

Store serves the same catalog and checkout workflow to the in-game browser and
the player dashboard. It resolves the item and currency against the server's
published offers and checks the quoted price. Accounts translates the resource
definitions into offers and inventory availability, then fulfills the selected
offer in an atomic garage transaction against the submitted revision.

Player Sessions publishes garage token verification and coordinates checkout
with other garage writes. Player Administration authenticates dashboard
sessions and supplies its HTTP transport to the shared Store view. Dashboard
catalog reads go directly to Accounts for the persisted garage snapshot.
Store owns no persistent state.

| Layer | Responsibilities |
| --- | --- |
| `domain/` | Match the submitted item and price to a server offer and reject unavailable or stale quotes. |
| `application/` | Read the authenticated player's catalog, dispatch checkout and project the updated Store view from the committed account. |
| `presentation/store/` | Catalog and checkout endpoints for both channels and the shared storefront components. |
| `infrastructure/` | Browser HTTP transport and native purchase notifications. |

[The Rust contract](src/lib.rs) publishes the Store service and routers. The
[web entry point](src/web.ts) publishes the standalone and embedded views and
the transport interface. See the [Store guide](../../docs/store.md) for offers,
configuration and the HTTP contract.

| Term | Meaning |
| --- | --- |
| Offer | An item or Premium Account package advertised at a server-owned price. |
| Quote | The expected item, price and account revision submitted at checkout. |
| Checkout | Resolve a quote, fulfill the selected offer, and return the updated catalog. |
| Catalog | The current offers with stock and availability for one player. |
