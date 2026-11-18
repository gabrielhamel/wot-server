# In-game Store

The garage’s Store tab opens the embedded page. The catalog follows the
regular EU offers mapped in [PR #45](https://github.com/gabrielhamel/wot-server/pull/45).
The garage HUD displays the account balances. The Store displays each offer’s
price, stock, and purchase eligibility.

| Tab | Offers |
| --- | --- |
| WoT Premium Account | 1, 3, 7, 14, 30, 90, 180 and 360 days for gold |
| Vehicles | Regular gold vehicles and Collectors’ vehicles |
| Exterior | 2D styles, 3D styles and crew skins |
| Service | Standard equipment, consumables and crew directives |
| Items for Bonds | Vehicles, improved equipment, equipment directives and styles |

The catalog excludes EUR checkout and WoT Plus subscriptions. Rotating offers,
fixed bundles whose contents or checkout prices are unverified, Personal
Reserves, and progressive styles such as Tiger Claw remain outside the current
catalog. Applying custom changes to an editable style is also outside the
current implementation.

## Catalog provenance

The resource bundle contains each nation’s `list.xml`, the Tech Tree nodes,
and regular service definitions. The Store selects Collectors’ vehicles by
the `collectorVehicle` tag, the resource credit price, and garage eligibility.
The observed fixture supplies 92 Collectors’ vehicles. The complete catalog
built from that fixture contains 355 offers, including 45 gold vehicles and
54 bond vehicles. The highest researched
Tech Tree tier in each nation gates that nation’s Collectors’ offers. The
server projects the same maximum into the native garage cache. Buying or
receiving a premium, reward or Collectors’ vehicle does not advance that maximum.

Gold and bond vehicle sale lists come from the live mapping. The server
resolves their entries to the installed resource definitions by catalog key.
Client gold base prices alone do not confirm sale eligibility. The server uses
client prices for standard and improved equipment, consumables and directives.
It uses the mapped web Store prices for crew skins and the mapped Bond Shop
prices for styles. The extractor brings over `artefacts.mo`,
`crew_skins.mo` and the British `gb_vehicles.mo` for localized display names.
Re-extract the bundle to use those catalogs. The catalog also accepts older
bundles with definition or mapped names as fallbacks.

Vehicle purchases require a free garage slot and add the stock vehicle.
Crew and ammunition can be acquired through the garage. Style purchases add
stock to the customization inventory. A style carries the client-defined
vehicle filter and can be applied to the common outfit of a compatible vehicle.
The server retains the client style descriptor and the client renders the
style from its local resources.

## WoT Premium Account

Checkout debits gold and persists the expiration timestamp in the same
revision as the purchase. Each package adds days to the later of the active
expiration time and the current server time. Each day is 86,400 seconds.
The native garage purchase command (`CMD_PREMIUM`, command 204 with arena
ID 0) and the browser checkout use the same package table and purchase rule.
The account cache and live account update both use the top-level `premium`
dictionary, with `PREMIUM_TYPE.PLUS` (2) as the active mask and expiry key.
The gateway sends an inactive update on expiry while the garage is connected.
The browser derives the active display from the returned timestamp.

This covers the garage entitlement lifecycle. Battle rewards, additional
XP, premium missions and the credit reserve depend on battle settlement
and remain unimplemented.

## Local preview

For frontend development, run `npm run dev` from the repository root and
open `/shop/` on the URL Vite prints. The [mock API](admin.md#frontend-development)
supplies a demo session and supports purchases against an in-memory balance.

## Configuration

Build the shared React project with `npm ci` and
`npm run build --workspace tools/web`. Configure the same HTTP listener used by the
[admin dashboard](admin.md). The Store can be enabled on its own.

```json
{
  "web": {
    "bind": "127.0.0.1:8080",
    "ui_dir": "../../tools/web/dist"
  },
  "store": {
    "origin": "http://127.0.0.1:8080"
  }
}
```

The client opens `/shop/` and sends requests to `/api/shop/`.
For remote clients, set `store.origin` to a reachable HTTPS origin and route
`/shop`, `/shop/*`, `/assets/*` and `/api/shop/*` to the shared listener
through your TLS reverse proxy. Keep admin routes on the access policy used
by your deployment. The container ships the compiled web UI in `web/`
beside the server binary.

If upgrading a development configuration that used a separate Store bind,
move the shared address to `web.bind` and update `store.origin` to match the
shared listener or its reverse proxy. Reconnect after changing the Store URL.

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
the Account shop-price cache requested by command 300. Both derive service item
prices from the same resource definitions and share the Premium Account
package table. The client research fixture
reported EU `v.2.4.1.1 #993`, launcher executable SHA-256
`fc80a8fcd804dba1ce54482d87950f4cbe31b3742eefd0c3de03dc28e941af7c`
and `win64/WorldOfTanks.exe` SHA-256
`58325ded3e2db7134638971e33de1eab09737f8b66401554d9723a2bf246bb11`.

The compiled client also reads `Account.initialServerSettings.igbWhitelist`
before the browser opens. When Store is enabled, the server sets `isEnabled`
to true and sends the configured hostname or IP as a single exact entry in
the Python `set` named `defaultWhitelist`. The client has no remote allowlist
URL to fetch for this server. In the installed executable above, the settings
reader at RVA `0x232fe70` reads these keys and checks `PySet_Type` before
iterating the entries. Without the host entry, the game log showed HTTP 418
for the embedded Store while the same URL returned HTTP 200 from the server
in an ordinary HTTP client.

The authenticated garage receives a random token as a URL fragment for the
Store page. URL fragments stay in the client. The page sends the token in the
`Authorization` header for same-origin requests. Gateway lookups require a
synchronized live garage session. The token expires with that session.
Redact authorization headers when logging proxy traffic. The game may log
its Store URL locally, including the fragment. Disconnect before sharing game
logs to invalidate the token.

- `GET /api/shop/catalog` returns offers with their categories, prices,
  eligibility and Depot quantities, the active Premium Account expiration,
  and the account revision.
- `POST /api/shop/orders` accepts an offer item ID, a `price` object
  with `currency` and `amount`, and the displayed account `revision` as
  a decimal string. The gateway holds the account’s write slot and reloads
  its persisted snapshot. Accounts verifies the quote and eligibility.
  PostgreSQL commits the debit, entitlement or inventory change and next
  revision in one transaction.
- The gateway publishes the committed diff to the garage. Replaying an order
  after commit returns HTTP 409. A concurrent request may return HTTP 503.
  Both outcomes require a fresh catalog before submitting another order.

## Purchase feedback

The selected purchase button shows a spinner for at least 350 ms while
checkout completes. After a successful checkout, the page sends a
`notification` command to the embedded browser’s `jsHostQuery` bridge.
The native client uses its purchase notification type for credits, gold
or bonds. Ordinary browsers update the offer state, and checkout failures
display an error on the page.

The fixture’s CEF subprocess and browser process expose `jsHostQuery`.
In the client reference, the Store registers `NotificationWebApi` in
`gui/Scaleform/daapi/view/lobby/store/browser/web_handlers.py`.
`web/web_client_api/ui/notification.py` routes the command through
`SystemMessages.pushMessage` to the native service channel.

## Verification

The PostgreSQL integration test
`store_purchase_updates_the_garage_and_rejects_replay` checks bootstrap,
authorization, stale and concurrent orders, insufficient funds, the
Collectors’ tier gate, credit and bond purchases, style compatibility,
Premium Account activation, extension and live expiry, native purchase,
reconnect persistence, and exported W3C parent and SQL/garage delivery
ancestry. The browser suite
exercises the page with MSW.

For real-client verification, open the Store from the garage, buy a Depot
item and confirm the stock and HUD balance. Buy Premium Account time and
check the garage account status before and after reconnecting. Buy a style
for a vehicle in the garage and apply it in Exterior.
