# In-game Store

The garage's Store tab opens the embedded Store page. The initial catalog
offers Improved Ventilation and Turbocharger in classes 1–3 for credits.
Each purchase adds one unmounted item to the player's Depot. The Store
shows the credit balance and Depot quantity for each offer.

## Configuration

Add an optional `store` object to the server's `server.json`. For a game
client on the same computer, use a loopback listener and origin.

```json
{
  "store": {
    "bind": "127.0.0.1:8081",
    "origin": "http://127.0.0.1:8081"
  }
}
```

For remote clients, set `origin` to a reachable HTTPS origin and route
`/store/*` on that origin to the Store listener through your TLS reverse proxy.
Keep the listener reachable only by the proxy. Serve the Store at the advertised
origin without redirects and isolate it from unrelated web applications.
The Store listener is independent of the optional admin listener.
Reconnect the game after enabling the Store so it receives its URL.

## Client and checkout protocol

In the pinned decompiled client, `ShopView` loads the URL resolved by
`shop_helpers.getShopURL` from `lobbyContext.getServerSettings().shop.hostUrl`.
`WebView` opens it using `BrowserController.load`, which expands URL macros
and can append browser parameters. These sources live under
`sources/res/scripts/client/` in [the client reference](references/README.md),
in `gui/Scaleform/daapi/view/lobby/store/browser/`,
`gui/Scaleform/daapi/view/lobby/shared/web_view.py`, and
`gui/game_control/BrowserController.py`. The server supplies `shop.hostUrl` in
`Account.initialServerSettings`. The catalog in this page is distinct from
the Account shop-price cache requested by command 300. Both derive equipment
prices from the same resource definitions. The client research fixture
reported EU `v.2.4.1.1 #993`, launcher executable SHA-256
`fc80a8fcd804dba1ce54482d87950f4cbe31b3742eefd0c3de03dc28e941af7c`
and `win64/WorldOfTanks.exe` SHA-256
`58325ded3e2db7134638971e33de1eab09737f8b66401554d9723a2bf246bb11`.

The authenticated garage receives a random token as a URL fragment for the
Store page. URL fragments stay in the client. The page sends the token in the
`Authorization` header for same-origin requests. Gateway lookups require a
synchronized live garage session. The token expires with that session.
Redact authorization headers when logging proxy traffic. The game may log
its Store URL locally, including the fragment. Disconnect before sharing game
logs to invalidate the token.

- `GET /store/catalog` returns the offers, balance, Depot quantities and
  account revision. Offer selection excludes items with `notInShop` or
  `notForSale` restrictions and uses the equipment rules' established price.
- `POST /store/orders` accepts an item descriptor, the displayed price in
  credits, and the displayed account revision as a decimal string. The gateway
  holds the account's write slot and reloads its persisted snapshot. Accounts
  checks the quote and applies the existing equipment purchase rules. PostgreSQL
  persists the credit debit, Depot stock and next revision in one transaction.
- The gateway publishes a committed inventory and balance diff to the garage.
  Replaying the same request after a commit returns HTTP 409. A concurrent
  request may get HTTP 503 while the write is pending. Both outcomes require
  loading fresh offers before submitting a new purchase. The revision check
  also rejects offers displayed before another account write.

## Verification

The PostgreSQL integration test
`store_purchase_updates_the_garage_and_rejects_replay` checks bootstrap,
catalog, authorization, changed prices, concurrent submission and replay,
insufficient credits, live garage replication, reconnect persistence, and
exported HTTP, Accounts, SQL and Mercury span ancestry.

For a real-client check, open Store from the garage, buy an offer and verify
that the credit balance falls by the displayed price. Open Depot and confirm
that the equipment count increased by one. Disconnect, sign back in and
verify both values again.
