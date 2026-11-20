# Contacts and private chat

The server enables the client's native XMPP messenger. Contacts search, saved
friends, accurate presence, private-window titles, offline delivery and history
use this path. No client mod or online-status spoofing is required. The previous
BW/BW_CHAT2 adapter remains bound for protocol tools and older sessions; its rooms
are ephemeral and do not share the native messenger's window state.

Player Sessions also uses BW_CHAT2 for live messages between members of a
[Training Room](wg-toolkit/session-flows.md#training-room-text-chat).

## Behavior and persistence

Players can find another nickname in Contacts and request friendship. The other
player receives a request and can accept or decline. Pending requests survive
disconnect and are delivered when the recipient connects. Acceptance adds both
players to each other's rosters and enables shared presence. A declined request
returns the sender's entry to an unconfirmed contact. Existing saved friends
retain their prior status. Send message is available even when the recipient is
offline.
The XMPP window title is the target player's canonical display name. Creating a
window does not notify the target; a message does. Closing a native window remains
a client-local action and never closes the other player's window.

Messages commit to PostgreSQL before live delivery. An offline recipient's messages
are available when they reconnect. Recent conversations are restored with their
peer's name. Login replays only messages from the peer; a conversation without
incoming messages gets a bodyless announcement identifying the peer. When the
client requests history, the server returns both sides' messages in chronological
order with the original sender, timestamp and stable message ID. The client uses
those IDs to reconcile received messages. There is no live echo back to the
sender: the native client already echoes its own text.

Retention is the **latest 100 messages per conversation**, including offline
messages. Up to 300 saved pairs per account and 16 recent conversations restored
at login bound storage and replay work. Opening another conversation rotates the
recent set; it does not delete that pair's saved history. Messages predating this
implementation were never stored and cannot be reconstructed. The initial
schema includes the social tables alongside accounts and inventory.

## Client evidence and compatibility

Source paths below are relative to
`docs/references/wot-src/sources/res/scripts/`. The decompiled tree
used for this research reports EU `v.2.4.1.1 #993`. The original executable
reference was `2.4.1.5498`.

Confirmed from supplied client sources:

- `client/gui/Scaleform/daapi/view/lobby/user_cm_handlers.py`,
  `UserContextMenuInfo.canCreateChannel`: the BW fallback requires `isOnline`.
  With XMPP enabled it instead checks the same-realm rule. This explains why the
  official service allows offline messages while the earlier fallback did not.
- `client/messenger/proto/__init__.py`, `ServerSettings.useToShowContacts`, and
  `proto/migration/contacts.py`: enabling XMPP changes Contacts to its native
  roster, search and private-message implementations.
- `client/messenger/proto/xmpp/XmppServerSettings.py`: `xmpp_enabled`, domain,
  resource and explicit `(host, port)` connections come from server settings.
- `client/messenger/proto/xmpp/connection.py`: JID node is the database ID;
  `ChatTokenResponse.getCredential` supplies the token obtained through the game
  connection. `common/constants.py`, **TOKEN_TYPE.XMPPCS = 1**. The unrelated
  RESTRICTION_SOURCE.XMPPCS value 11 is not the token type.
- `client/messenger/proto/xmpp/entities.py`, `_XMPPChannelEntity.getFullName`:
  returns the peer name without the BW owner's-name suffix. BW's
  `proto/bw/entities.py` appends `(ownerName)` for non-system channels.
- `proto/xmpp/extensions/search.py`, `spa_resolver.py`: WG nickname-prefix query
  and ID/nickname resolver rows use `http://wargaming.net/xmpp#spa-resolver`.
- `proto/xmpp/extensions/chat.py`, `messages/manager.py` and
  `messages/chat_session.py`: private history uses an IQ query in
  `http://wargaming.net/xmpp#private-message-history`, with `with` and `limit`.
  The first-open toast came from the server's `item-not-found` fallback.
  A successful response sends an IQ result followed by chat stanzas from
  the server domain to the bound resource. Each `private-history` extension
  carries the IQ's `request-id`, and the final item has `final='true'`. An
  empty conversation gets a bodyless final item.
  A trace from client `2.4.1.5514` showed that a peer-addressed final item
  reached Python with the correct request ID and final flag, but never
  reached the private history handler. `messages/manager.py` tests the
  sender's bare JID against the MUC provider before routing history.
  That provider inherits a lookup across all XMPP channels
  (`messages/provider.py` and `find_criteria.py`), including the open
  private window. Using the server's JID avoids that match. The saved author
  remains in each message's metadata. A repeat trace confirmed the private
  history handler ran for both an empty and a populated conversation, and
  the player confirmed they could type and send.
  The latest 16 conversations are also restored at login. Requested history
  uses the same message UUIDs so the client can reconcile it with unread messages.
  Sender metadata is `wgexts/userid/nickname`; message IDs use WG's `message-id`
  extension; timestamps use `urn:xmpp:delay` UTC stamps.
- `proto/xmpp/messages/provider.py`: outgoing text is echoed locally before the
  XMPP send. `messages/chat_session.py` restores channels from client cache and
  reconciles unread/history messages using UUIDs. Restoring a peer channel with
  empty body creates its entry without inventing another message.
- `messages/chat_session.py`, `ChatSessionsProvider.addMessage`, uses
  `wgexts/userid/nickname` on the first ordinary chat stanza to initialize the
  contact ID and title. The stanza's `from` JID selects the channel but does not
  override that metadata. Replaying a self-authored message as ordinary chat
  therefore caused the reported self-named window and duplicate player names.
  The server sends self-authored messages only through the history response.
  `entities.py`, `XMPPChatChannelEntity.getPersistentState`, caches the title
  and contact ID; close and reopen a previously misnamed window to recreate it.
- `proto/xmpp/contacts/{roster_tasks,block_tasks,note_tasks}.py`: roster,
  block-list and empty contact-notes initialization must all finish before
  incoming private messages are released. The service supports all three reads.

Native reconnect traces from both local and remote clients showed SASL and
resource binding succeeding, followed by a GET in
`http://wargaming.net/xmpp#filter-roster`. The executable contains that namespace
and nearby roster item parsing strings. Returning an unsupported-feature error
at this point left Contacts on **Connecting**. The server now serves saved
friends under the requested roster namespace. Standard `jabber:iq:roster`
remains available for roster changes and ordinary XMPP tools.

The supplied `win64/WorldOfTanks.exe` contains SASL PLAIN/DIGEST-MD5 and Gloox
error strings, corroborating the native XMPP transport. The implemented handshake
is XMPP 1.0, SASL PLAIN and resource binding. After the roster fix, live local
and remote game clients advanced through the filtered roster, block list and
notes requests and kept their chat sockets open. After the nickname fix, a live local client opened the private chat window
and sent a history request. Tooltip rendering and offline delivery still need
separate in-game verification. Historical BigWorld 14.4.1 is a
framing reference, not evidence for modern XMPP application semantics.

Native friendship starts with a roster SET and then a presence subscription
request. `contacts/sub_tasks.py` waits for a roster push whose subscription is
pending (`subscription='none' ask='subscribe'`) or approved
(`subscription='both'`). The recipient's request notification is driven by
incoming `subscribe` presence with the requester's nickname.
`subscribed` approves a persisted incoming request; `unsubscribed` declines it.
The native contact table bounds pending inbound requests at 100 per recipient.
The BW adapter retains its original contact operations.

The native Blacklist reads the canonical nickname from the nested
`wgexts/nickname` extension on block-list items and block or unblock pushes
(`extensions/contact_item.py` and `contacts/block_tasks.py`). A bare JID left
the nickname empty and prevented name-validated actions such as adding the
player again. The server supplies this metadata from the account directory.

A direct message blocked in either direction is discarded before saving or
forwarding. For game compatibility this is silent: the stock client displays
the same generic retry toast for message stanza errors and already echoes its
outgoing text locally. A local echo is therefore not a delivery receipt.
If either player requests history while the block is active, the server sends
a successful empty history response. The client can open the window without
showing a retry toast. Saved messages become available again after unblocking.

The game recognizes `wot` as the Tanks XMPP resource. With that resource,
`contacts_vo_converter.py` chooses the green online icon and formats the tooltip
from the presence extension `wgexts-client/game-host`. The client resolves the
host by exact URL against its own server list and displays the configured short
name. In this single-world server, presence uses the receiving client's game
host reported through `contacts/__init__.py`; loopback and remote users can
therefore have different URLs for the same world. An initial presence without
metadata does not suppress the next presence that supplies the host.

The EU client validates the target nickname locally before opening a chat
(`messages/manager.py`, `messenger/ext/__init__.py` and
`common/external_strings_utils.py`). Use 2–24 ASCII letters, digits or underscores
for local demo accounts intended to use private chat. Spaces caused
`startChatSession: NAME_INVALID` in the live client, with no request sent to the
server.

## Native endpoint and authentication

Player Sessions binds TCP on the **same address/port as BaseApp UDP** (default TCP 20017).
The client receives the same route-selected or explicitly advertised endpoint,
including the localhost shortcut. The XMPP virtual domain is `wot-server.local`;
explicit connection settings supply its reachable IP, so DNS is not required.
Remote clients need TCP forwarding/firewall access as well as the existing UDP
ports. An `advertised_base` override must expose both transports at that port.

The authenticated Account's token request type 1 mints 256 random bits. Tokens
bind the account to its game-session source IP and admit a chat connection for
60 seconds after issuance. SASL cannot authenticate a different account with
another player's token. Active chat authorization is revoked on game disconnect;
issuing a replacement token invalidates the old chat credential. Only one XMPP
resource per account is admitted. When the server closes an XMPP stream, it
finishes connection cleanup and releases the admission slot before closing the
socket. XMPP shutdown follows the game gateway.

The local endpoint currently advertises PLAIN **without STARTTLS**. It carries a
short-lived game-issued credential, not the account password, but the TCP leg is
not encrypted. Keep this TCP port on localhost or a trusted private network until
TLS is implemented. TLS, federation, BOSH, MUC, groups, notes editing, and external identities are not implemented. Unsupported IQ operations
return explicit stanza errors; only an empty contact-notes list is supported.

If Contacts remains on **Connecting**, use the server's `debug` log level and
reconnect the game client. Look for the accepted TCP connection, the XMPP stream
header, the game-issued chat token, and the roster, block-list and notes IQs.
The diagnostics log protocol stages rather than credentials or chat text. A
successful TCP connection alone does not establish that Contacts initialized.

## Boundaries, limits and failure behavior

[Social](../src/social/README.md) owns directory-backed contacts, durable private
pairs and message history. Its PostgreSQL adapter serializes both participants in
ID order, checks ignores in both directions within the write transaction, enforces
saved-pair and retention limits, and commits before reporting success. Account
names come from Accounts' published `PlayerDirectory` API. Supplied `from`
attributes are never trusted as message authors.

[Player Sessions](../src/player_sessions/README.md) owns XMPP framing, token admission, roster and WG
extension translation, presence fanout and live sockets. The TCP task limit equals
`max_clients`; each session processes one stanza at a time. Incoming XML is bounded
to 32 KiB, depth 16, and 64 socket reads per incomplete frame. DTDs are rejected.
The shared request budget admits 32 requests per second; presence changes have a
one-second interval; sends have a 500 ms cooldown and 2048-byte, nonblank UTF-8
body limit. Database/stanza processing and writes have ten-second deadlines;
unauthenticated connections expire after 30 seconds. Outbound live queues hold
128 stanzas per peer; overflow evicts that connection, with committed text still
available in history. Contact searches return at most 50 literal-prefix matches;
block-list and roster operations remain bounded to 300 contacts.

A bad XML frame, invalid credential, unauthorized recipient, ignored contact,
queue overflow or recoverable database failure rejects that operation or closes
that socket. It never terminates the gateway. Native sessions are asynchronous;
SQL does not block the UDP loop. Presence observer queries include only the bounded
online set. Retention and recent-conversation rotation are transactional.

## Legacy BW compatibility

The BW fallback remains available through `chatCommandFromClient`; search uses
`messenger_onActionByClient_chat2` action 2. Both bound method dictionaries come
from `entity_defs/interfaces/{Chat,PlayerMessenger_chat2}.def` and `alias.xml`.
`client_common/ClientChat.py`, `common/chat_shared.py` and
`proto/bw/{UsersManager,ChannelsManager}.py` supply its request/event shapes.

| Operation | BW command | BW response event |
| --- | ---: | ---: |
| Add friend / ignored | 3 / 4 | 15 / 14 |
| Remove friend / ignored | 5 / 6 | 17 / 16 |
| Open private room | 7 | 6 then 7 to requester only |
| Read roster / presence | 9 / 10 | 13 / 19 |
| Read private members | 36 | 3 |
| Enter / leave private room | 37 / 38 | 6 then 7 / local 8 |
| Send private text | 39 | Initialize recipient on first message; event 1 to both |

These legacy rooms retain independent subscriptions, cap 16 rooms per player and
are reclaimed on both subscriptions closing or either game session disconnecting.
The native service, rather than a client patch or fake online presence, provides
the requested offline and persistent behavior.

## Verification

The user confirmed the earlier two-client BW messenger and independent-window
fixes. The new native service has protocol/database verification and live
local and remote Contacts initialization traces. In the live friend-request
replay, removing the old entries and sending a new request from the remote
client led to approval from the local client and two accepted database entries.
The integration test also exercises offline replay and declining before approval.

`tests/support/xmpp_flow.rs`, invoked by the opt-in garage integration test, obtains
type-1 tokens over encrypted UDP, rejects an account/token mismatch, performs the
SASL/bind/roster handshake, sends while the recipient has no chat connection,
reconnects them, checks that a conversation announcement arrives before the
history request, and verifies canonical peer metadata and replies. It checks
that reconnecting with sent-only history announces the peer without a message,
then sends an offline reply to that account and checks that its mixed history
initializes the window with the peer and preserves both authors in history.
Sending too quickly returns a stanza
error and keeps the chat connected. A DTD client is rejected
before another authenticated client succeeds. A forged message `from` still
persists the authenticated author. A fresh SQLx pool verifies durability,
100-message retention, invalid body rejection and ignored-recipient rejection.
Both plaintext and RSA-encrypted game login variants run the same checks.

`tests/social.rs` and `tests/support/social_flow.rs` retain BW subscription,
independent-window, presence and malformed-message regression coverage. XML unit
tests cover partial/coalesced frames, depth and DTD rejection. Commands are in
[Testing](testing.md). To validate in game, reconnect both clients, verify the peer
name as the title, send with one client disconnected, and log that client back in
to verify its conversation and history. Also check that opening an unsent window
and closing either side remain local actions.
