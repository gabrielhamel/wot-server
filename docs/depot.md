# Garage Depot

The native Depot reads the same garage inventory as the tank setup panels.
Unloading shells, swapping modules and demounting equipment make those items
available in the Depot. Credit-priced consumables bought through the shop also
appear there. The client sorts stock into its native inventory and sale tabs
according to compatibility with the account's vehicles.

## Client contract

The Account bootstrap sends `initialServerSettings["shop"]["isStorageEnabled"]`
as true. The Depot uses the normal Account sync and shop prices.

| Command | Arguments | Behavior |
| --- | --- | --- |
| `CMD_SELL_ITEM` 305, `doCmdInt4` | Shop revision, item type, item compact descriptor, count | Sell one unmounted stock entry. |
| `CMD_SELL_MULTIPLE_ITEMS` 310, `doCmdIntArr` | Shop revision followed by up to 80 triples of item type, compact descriptor and count | Sell all lines in the request in one transaction. |

Accounts validates the descriptor, item type, quantity, catalog price and
available stock. Repeated lines draw from the same remaining quantity. Items
tagged `notForSale` are rejected. The shop price determines the per-item credit
proceeds, rounded up at the advertised 50% sale factor after conversion from
gold or bonds when applicable. Experimental Equipment uses the same commands
to [disassemble Depot stock](equipment.md#client-contract) and pays the full
catalog price in Components. A batch may contain both kinds of proceeds.
Sales that would pay other unsupported currencies are rejected.

The existing revision-checked PostgreSQL transaction writes stock and proceeds
together. After commit the server sends an Account update with the new revision,
the new credit and Components balances, and the affected inventory tables. A partial sale sends the remaining
count. A depleted entry is sent as `None`. Mounted equipment, loaded shells and
installed modules are outside the stock used by sales.

Vehicle and customization sales, consumable loadouts, and reward inventories
such as personal reserves have separate client flows.

## Compatibility evidence

In the decompiled client, `client/helpers/server_settings.py` reads the
`shop` setting. `client/gui/Scaleform/daapi/view/lobby/storage/storage_view.py`
uses it to choose between the Depot and the unavailable screen.
`client/account_helpers/Inventory.py` sends the sale commands defined in
`common/AccountCommands.py`. `client/account_helpers/Shop.py:getSellPrice`
calculates the displayed sale prices. The supplied 2.4.1.5498 client's
`res/packages/scripts.pkg` contains the matching settings, Depot and sale
command symbols.

The [garage integration test](testing.md) exercises bootstrap, inventory sync
and sales over the encrypted Account connection using those installed resources.
