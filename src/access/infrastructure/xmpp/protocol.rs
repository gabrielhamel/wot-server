use super::{DOMAIN, Online, Outbound, RESOURCE, Shared};
use crate::social::{
    ChatMessage, ContactChange, Friendship, FriendshipAction, Player, PrivateConversationBlocked,
    RequestBudget,
};
use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use rsa::rand_core::{OsRng, RngCore};
use std::{
    collections::{HashSet, VecDeque},
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

const WG_CLIENT: &str = "http://wargaming.net/xmpp#client";
const WG: &str = "http://wargaming.net/xmpp#v2";
const FILTER_ROSTER: &str = "http://wargaming.net/xmpp#filter-roster";
const RESOLVER: &str = "http://wargaming.net/xmpp#spa-resolver";
const HISTORY: &str = "http://wargaming.net/xmpp#private-message-history";
pub(super) fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
}
fn jid(id: i64) -> String {
    format!("{id}@{DOMAIN}")
}
fn player_id(value: &str) -> Result<i64> {
    let bare = value.split('/').next().context("Empty JID")?;
    let (node, domain) = bare.split_once('@').context("Missing JID domain")?;
    anyhow::ensure!(domain == DOMAIN, "Foreign XMPP domain");
    let id: i64 = node.parse()?;
    anyhow::ensure!(id > 0, "Invalid account ID");
    Ok(id)
}
fn info(player: &Player) -> String {
    format!(
        "<wgexts xmlns='{WG}'><userid>{}</userid><nickname>{}</nickname></wgexts>",
        player.id,
        xml(&player.name)
    )
}
pub(super) fn presence(player: &Player, available: bool, host: Option<&str>) -> String {
    let host = host.filter(|_| available).map_or_else(String::new, |host| {
        format!(
            "<wgexts-client xmlns='{WG_CLIENT}'><game-host>{}</game-host></wgexts-client>",
            xml(host)
        )
    });
    format!(
        "<presence from='{}/{RESOURCE}'{}>{}{host}</presence>",
        jid(player.id),
        if available { "" } else { " type='unavailable'" },
        info(player)
    )
}
fn block_item(player: &Player) -> String {
    format!("<item jid='{}'>{}</item>", jid(player.id), info(player))
}

fn roster_item(player: &Player, state: Option<Friendship>) -> String {
    let attributes = match state {
        None => "subscription='remove'",
        Some(Friendship::Saved) => "subscription='none'",
        Some(Friendship::Pending) => "subscription='none' ask='subscribe'",
        Some(Friendship::Accepted) => "subscription='both'",
    };
    format!(
        "<item jid='{}' name='{}' {attributes}/>",
        jid(player.id),
        xml(&player.name)
    )
}

fn subscription_request(requester: &Player, recipient: i64) -> String {
    format!(
        "<presence from='{}' to='{}' type='subscribe'><nick xmlns='http://jabber.org/protocol/nick'>{}</nick>{}</presence>",
        jid(requester.id),
        jid(recipient),
        xml(&requester.name),
        info(requester)
    )
}
fn history_marker(request: &str, final_item: bool) -> String {
    format!(
        "<private-history xmlns='{HISTORY}' request-id='{}' final='{final_item}'/>",
        xml(request)
    )
}

fn message(recipient: i64, message: &ChatMessage, history: Option<(&str, bool)>) -> String {
    let stamp = chrono::DateTime::from_timestamp(message.sent_at as i64, 0)
        .map(|time| time.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default();
    let (from, to, id, marker) = match history {
        Some((request, final_item)) => (
            DOMAIN.to_owned(),
            format!("{}/{RESOURCE}", jid(recipient)),
            format!("history-{}-{}", xml(request), message.id),
            history_marker(request, final_item),
        ),
        None => (
            jid(message.sender.id),
            jid(recipient),
            format!("wg-{}", message.id),
            String::new(),
        ),
    };
    format!(
        "<message type='chat' id='{id}' from='{from}' to='{to}'><body>{}</body>{}<delay xmlns='urn:xmpp:delay' stamp='{stamp}'/><message-id xmlns='http://wargaming.net/xmpp#message-id' uuid='wg-{}'/>{marker}</message>",
        xml(&message.text),
        info(&message.sender),
        message.id
    )
}
fn result(id: &str, inner: &str) -> String {
    format!(
        "<iq type='result' id='{}' from='{DOMAIN}'>{inner}</iq>",
        xml(id)
    )
}
fn error(id: &str, condition: &str) -> String {
    format!(
        "<iq type='error' id='{}' from='{DOMAIN}'><error type='cancel'><{condition} xmlns='urn:ietf:params:xml:ns:xmpp-stanzas'/></error></iq>",
        xml(id)
    )
}

fn message_error(
    owner: i64,
    peer: i64,
    node: roxmltree::Node<'_, '_>,
    kind: &str,
    condition: &str,
) -> Vec<String> {
    vec![format!(
        "<message type='error' from='{}' to='{}' id='{}'><error type='{kind}'><{condition} xmlns='urn:ietf:params:xml:ns:xmpp-stanzas'/></error></message>",
        jid(peer),
        jid(owner),
        xml(node.attribute("id").unwrap_or_default()),
    )]
}

pub(super) struct Connection {
    shared: Arc<Shared>,
    ip: IpAddr,
    sender: mpsc::Sender<Outbound>,
    pub identity: Option<(i64, String)>,
    pub generation: u64,
    pub budget: RequestBudget,
    bound: bool,
    ready: bool,
    next_message: Instant,
    next_presence: Instant,
    delivered: HashSet<i64>,
    delivered_order: VecDeque<i64>,
}
impl Connection {
    pub fn new(shared: Arc<Shared>, ip: IpAddr, sender: mpsc::Sender<Outbound>) -> Self {
        Self {
            shared,
            ip,
            sender,
            identity: None,
            generation: OsRng.next_u64(),
            budget: Default::default(),
            bound: false,
            ready: false,
            next_message: Instant::now(),
            next_presence: Instant::now(),
            delivered: HashSet::new(),
            delivered_order: VecDeque::new(),
        }
    }
    fn remember_message(&mut self, id: i64) -> bool {
        if !self.delivered.insert(id) {
            return false;
        }
        self.delivered_order.push_back(id);
        if self.delivered_order.len() > 4096
            && let Some(oldest) = self.delivered_order.pop_front()
        {
            self.delivered.remove(&oldest);
        }
        true
    }

    fn deliver_incoming(&mut self, owner: i64, saved: &ChatMessage) -> Option<String> {
        (saved.sender.id != owner && self.remember_message(saved.id))
            .then(|| message(owner, saved, None))
    }

    pub(super) fn outbound(&mut self, outbound: Outbound) -> Option<String> {
        match outbound {
            Outbound::Stanza(stanza) => Some(stanza),
            Outbound::Direct(message) => {
                let owner = self.identity.as_ref()?.0;
                self.deliver_incoming(owner, &message)
            }
        }
    }
    pub fn bound(&self) -> bool {
        self.bound
    }
    pub async fn handle(&mut self, stanza: &str) -> Result<Vec<String>> {
        let wrapped = format!(
            "<root xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams'>{stanza}</root>"
        );
        let document = roxmltree::Document::parse(&wrapped)?;
        let node = document
            .root_element()
            .first_element_child()
            .context("Empty XMPP stanza")?;
        tracing::debug!(element = node.tag_name().name(), "Handling XMPP element");
        if node.tag_name().name() == "auth" {
            anyhow::ensure!(self.identity.is_none(), "Repeated authentication");
            anyhow::ensure!(
                node.tag_name().namespace() == Some("urn:ietf:params:xml:ns:xmpp-sasl")
                    && node.attribute("mechanism") == Some("PLAIN"),
                "Unsupported SASL mechanism"
            );
            let decoded = STANDARD.decode(node.text().unwrap_or_default())?;
            let parts: Vec<_> = decoded.split(|b| *b == 0).collect();
            anyhow::ensure!(parts.len() == 3, "Malformed SASL identity");
            let username = std::str::from_utf8(parts.get(1).context("Missing SASL username")?)?;
            let id: i64 = username
                .split('@')
                .next()
                .context("Empty SASL username")?
                .parse()?;
            let credential = std::str::from_utf8(parts.get(2).context("Missing SASL credential")?)?;
            let authz = std::str::from_utf8(parts.first().context("Missing SASL authorization")?)?;
            if !self.shared.authorized(id, credential, self.ip, true)
                || (!authz.is_empty() && authz != jid(id))
            {
                return Ok(vec![
                    "<failure xmlns='urn:ietf:params:xml:ns:xmpp-sasl'><not-authorized/></failure>"
                        .into(),
                ]);
            }
            self.identity = Some((id, credential.into()));
            return Ok(vec![
                "<success xmlns='urn:ietf:params:xml:ns:xmpp-sasl'/>".into(),
            ]);
        }
        let (owner, _) = self
            .identity
            .as_ref()
            .context("Unauthenticated XMPP stanza")?;
        let owner = *owner;
        match node.tag_name().name() {
            "iq" => self.iq(owner, node).await,
            "presence" => {
                anyhow::ensure!(self.bound, "XMPP resource is not bound");
                if let Some(to) = node.attribute("to") {
                    let action = match node.attribute("type") {
                        Some("subscribe") => FriendshipAction::Request,
                        Some("subscribed") => FriendshipAction::Accept,
                        Some("unsubscribed") => FriendshipAction::Decline,
                        Some("unsubscribe") => FriendshipAction::Cancel,
                        _ => return Ok(Vec::new()),
                    };
                    let peer = player_id(to)?;
                    return match self.change_subscription(owner, peer, action).await {
                        Ok(replies) => Ok(replies),
                        Err(error) => {
                            tracing::debug!(owner, peer, %error, "Friendship action rejected");
                            Ok(vec![format!(
                                "<presence type='error' from='{}' to='{}'><error type='cancel'><not-allowed xmlns='urn:ietf:params:xml:ns:xmpp-stanzas'/></error></presence>",
                                jid(peer),
                                jid(owner)
                            )])
                        }
                    };
                }
                let available = node.attribute("type") != Some("unavailable");
                let host = node
                    .children()
                    .find(|child| child.has_tag_name((WG_CLIENT, "wgexts-client")))
                    .and_then(|ext| {
                        ext.children()
                            .find(|child| child.has_tag_name((WG_CLIENT, "game-host")))
                    })
                    .and_then(|host| host.text())
                    .filter(|host| {
                        !host.is_empty()
                            && host.len() <= 255
                            && !host.chars().any(char::is_whitespace)
                    });
                {
                    let mut online = self.shared.online.lock().await;
                    let peer = online.get_mut(&owner).context("Missing XMPP session")?;
                    let first_host = available && host.is_some() && peer.game_host.is_none();
                    if Instant::now() < self.next_presence && !first_host {
                        return Ok(Vec::new());
                    }
                    if available && let Some(host) = host {
                        peer.game_host = Some(host.to_owned());
                    }
                    peer.present = available;
                }
                self.next_presence = Instant::now() + Duration::from_secs(1);
                self.shared.presence(owner, available).await?;
                if available && !self.ready {
                    let rooms = self.shared.social.restore(owner).await?;
                    self.ready = true;
                    tracing::debug!(owner, conversations = rooms.len(), "XMPP session ready");
                    let mut replies: Vec<String> = self
                        .shared
                        .social
                        .inbound_requests(owner)
                        .await?
                        .iter()
                        .map(|requester| subscription_request(requester, owner))
                        .collect();
                    for room in rooms {
                        let Some(peer) = room.peer(owner) else {
                            continue;
                        };
                        let before = replies.len();
                        replies.extend(
                            room.history
                                .iter()
                                .filter_map(|saved| self.deliver_incoming(owner, saved)),
                        );
                        if replies.len() == before {
                            replies.push(format!(
                                "<message type='chat' from='{}' to='{}'>{}</message>",
                                jid(peer.id),
                                jid(owner),
                                info(peer)
                            ));
                        }
                    }
                    return Ok(replies);
                }
                Ok(Vec::new())
            }
            "message" => {
                anyhow::ensure!(self.bound, "XMPP resource is not bound");
                let body = node
                    .children()
                    .find(|n| n.has_tag_name(("jabber:client", "body")))
                    .and_then(|n| n.text());
                let Some(body) = body else {
                    return Ok(Vec::new());
                };
                let peer = player_id(node.attribute("to").context("Missing message recipient")?)?;
                if Instant::now() < self.next_message {
                    return Ok(message_error(
                        owner,
                        peer,
                        node,
                        "wait",
                        "resource-constraint",
                    ));
                }
                let result = async {
                    let room = self.shared.social.open(owner, peer).await?;
                    self.shared.social.send(owner, room.id, body).await
                }
                .await;
                if result
                    .as_ref()
                    .is_err_and(|error| error.is::<PrivateConversationBlocked>())
                {
                    tracing::debug!(owner, peer, "Dropped blocked private message");
                    return Ok(Vec::new());
                }
                self.next_message = Instant::now() + Duration::from_millis(500);
                match result {
                    Ok(room) => {
                        if let Some(saved) = room.history.last() {
                            self.remember_message(saved.id);
                            self.shared
                                .enqueue(peer, Outbound::Direct(saved.clone()))
                                .await;
                        }
                        Ok(Vec::new())
                    }
                    Err(error) => {
                        tracing::debug!(owner,peer,%error,"Private message rejected");
                        Ok(message_error(owner, peer, node, "cancel", "not-allowed"))
                    }
                }
            }
            _ => Ok(Vec::new()),
        }
    }

    fn roster_push(&self, owner: i64, player: &Player, state: Option<Friendship>) -> String {
        format!(
            "<iq type='set' id='roster-{}' to='{}'><query xmlns='jabber:iq:roster'>{}</query></iq>",
            self.generation,
            jid(owner),
            roster_item(player, state)
        )
    }

    async fn roster_state(&self, owner: i64, peer: i64) -> Result<Option<Friendship>> {
        Ok(self
            .shared
            .social
            .roster(owner)
            .await?
            .into_iter()
            .find(|contact| contact.player.id == peer && !contact.ignored)
            .map(|contact| contact.friendship))
    }

    async fn change_subscription(
        &self,
        owner: i64,
        peer: i64,
        action: FriendshipAction,
    ) -> Result<Vec<String>> {
        let changed = self.shared.social.friendship(owner, peer, action).await?;
        let other = self
            .shared
            .social
            .player(peer)
            .await?
            .context("Unknown contact")?;
        let current = self
            .shared
            .social
            .player(owner)
            .await?
            .context("Unknown account")?;
        match action {
            FriendshipAction::Request if changed => {
                self.shared
                    .push_available(peer, subscription_request(&current, peer))
                    .await;
                Ok(vec![self.roster_push(
                    owner,
                    &other,
                    Some(Friendship::Pending),
                )])
            }
            FriendshipAction::Request => {
                let state = self.roster_state(owner, peer).await?;
                Ok(vec![self.roster_push(owner, &other, state)])
            }
            FriendshipAction::Accept if changed => {
                self.shared
                    .push(
                        peer,
                        self.roster_push(peer, &current, Some(Friendship::Accepted)),
                    )
                    .await;
                self.shared
                    .push(peer, self.shared.contact_presence(peer, &current).await)
                    .await;
                Ok(vec![
                    self.roster_push(owner, &other, Some(Friendship::Accepted)),
                    self.shared.contact_presence(owner, &other).await,
                ])
            }
            FriendshipAction::Decline if changed => {
                self.shared
                    .push(
                        peer,
                        self.roster_push(peer, &current, Some(Friendship::Saved)),
                    )
                    .await;
                Ok(Vec::new())
            }
            FriendshipAction::Cancel if changed => Ok(vec![self.roster_push(
                owner,
                &other,
                Some(Friendship::Saved),
            )]),
            _ => Ok(Vec::new()),
        }
    }

    async fn iq(&mut self, owner: i64, node: roxmltree::Node<'_, '_>) -> Result<Vec<String>> {
        let id = node.attribute("id").unwrap_or_default();
        let kind = node.attribute("type").unwrap_or_default();
        if kind == "result" || kind == "error" {
            return Ok(Vec::new());
        }
        let Some(query) = node.first_element_child() else {
            return Ok(vec![error(id, "bad-request")]);
        };
        let namespace = query.tag_name().namespace().unwrap_or_default();
        tracing::debug!(owner, namespace, kind, "Handling XMPP IQ");
        if namespace == "urn:ietf:params:xml:ns:xmpp-bind" {
            anyhow::ensure!(!self.bound, "Repeated XMPP bind");
            let mut online = self.shared.online.lock().await;
            anyhow::ensure!(!online.contains_key(&owner), "Duplicate XMPP session");
            online.insert(
                owner,
                Online {
                    generation: self.generation,
                    sender: self.sender.clone(),
                    present: false,
                    game_host: None,
                },
            );
            self.bound = true;
            return Ok(vec![result(
                id,
                &format!(
                    "<bind xmlns='{namespace}'><jid>{}/{RESOURCE}</jid></bind>",
                    jid(owner)
                ),
            )]);
        }
        anyhow::ensure!(self.bound, "XMPP resource is not bound");
        let response = self.handle_query(owner, id, kind, query).await;
        match response {
            Ok(replies) => Ok(replies),
            Err(error_value) => {
                tracing::debug!(owner,%namespace,error=%error_value,"XMPP query rejected");
                Ok(vec![error(id, "not-allowed")])
            }
        }
    }
    async fn handle_query(
        &mut self,
        owner: i64,
        id: &str,
        kind: &str,
        query: roxmltree::Node<'_, '_>,
    ) -> Result<Vec<String>> {
        let namespace = query.tag_name().namespace().unwrap_or_default();
        match namespace {
            "urn:ietf:params:xml:ns:xmpp-session" | "urn:xmpp:ping" => Ok(vec![result(id, "")]),
            FILTER_ROSTER if kind != "get" => Ok(vec![error(id, "feature-not-implemented")]),
            "jabber:iq:roster" | FILTER_ROSTER => {
                if kind == "set" {
                    let item = query.first_element_child().context("Missing roster item")?;
                    let peer = player_id(item.attribute("jid").context("Missing roster JID")?)?;
                    let remove = item.attribute("subscription") == Some("remove");
                    self.shared
                        .social
                        .friendship(
                            owner,
                            peer,
                            if remove {
                                FriendshipAction::Remove
                            } else {
                                FriendshipAction::Save
                            },
                        )
                        .await?;
                    let player = self
                        .shared
                        .social
                        .player(peer)
                        .await?
                        .context("Unknown roster account")?;
                    let state = self.roster_state(owner, peer).await?;
                    if remove {
                        let current = self
                            .shared
                            .social
                            .player(owner)
                            .await?
                            .context("Unknown account")?;
                        let reverse = self.roster_state(peer, owner).await?;
                        self.shared
                            .push(peer, self.roster_push(peer, &current, reverse))
                            .await;
                        self.shared
                            .push(peer, presence(&current, false, None))
                            .await;
                    }
                    return Ok(vec![
                        result(id, ""),
                        self.roster_push(owner, &player, state),
                    ]);
                }
                let contacts = self.shared.social.roster(owner).await?;
                let items = contacts
                    .iter()
                    .filter(|c| !c.ignored)
                    .map(|c| roster_item(&c.player, Some(c.friendship)))
                    .collect::<String>();
                Ok(vec![result(
                    id,
                    &format!("<query xmlns='{namespace}'>{items}</query>"),
                )])
            }
            "urn:xmpp:blocking" => {
                let tag = query.tag_name().name();
                if kind == "set" && !matches!(tag, "block" | "unblock") {
                    return Ok(vec![error(id, "feature-not-implemented")]);
                }
                if kind == "set" {
                    let items: Vec<_> = query
                        .children()
                        .filter(|n| n.is_element())
                        .take(301)
                        .collect();
                    anyhow::ensure!(
                        (1..=300).contains(&items.len()),
                        "Invalid block-list request size"
                    );
                    let mut updated = String::new();
                    for item in items {
                        let peer = player_id(item.attribute("jid").context("Missing block JID")?)?;
                        let player = self
                            .shared
                            .social
                            .player(peer)
                            .await?
                            .context("Unknown blocked account")?;
                        self.shared
                            .social
                            .change(
                                owner,
                                peer,
                                if tag == "unblock" {
                                    ContactChange::RemoveIgnored
                                } else {
                                    ContactChange::Ignore
                                },
                            )
                            .await?;
                        updated.push_str(&block_item(&player));
                    }
                    return Ok(vec![
                        result(id, ""),
                        format!(
                            "<iq type='set' id='block-{}'><{tag} xmlns='urn:xmpp:blocking'>{updated}</{tag}></iq>",
                            self.generation,
                        ),
                    ]);
                }
                let roster = self.shared.social.roster(owner).await?;
                let items = roster
                    .iter()
                    .filter(|c| c.ignored)
                    .map(|c| block_item(&c.player))
                    .collect::<String>();
                Ok(vec![result(
                    id,
                    &format!("<blocklist xmlns='urn:xmpp:blocking'>{items}</blocklist>"),
                )])
            }
            RESOLVER => {
                let mut players = Vec::new();
                let tag = query.tag_name().name();
                if tag == "nickname-prefix-search" {
                    players = self
                        .shared
                        .social
                        .search(query.attribute("prefix").unwrap_or_default(), None)
                        .await?;
                } else {
                    let items: Vec<_> = query
                        .children()
                        .filter(|n| n.is_element())
                        .take(51)
                        .collect();
                    anyhow::ensure!(items.len() <= 50, "Resolver request limit exceeded");
                    for item in items {
                        if let Some(id) = item.attribute("id") {
                            if let Some(player) = self.shared.social.player(id.parse()?).await? {
                                players.push(player);
                            }
                        } else if let Some(nickname) = item.attribute("nickname") {
                            players.extend(
                                self.shared
                                    .social
                                    .search(nickname, None)
                                    .await?
                                    .into_iter()
                                    .filter(|p| p.name.eq_ignore_ascii_case(nickname)),
                            );
                        }
                    }
                }
                let limit = query
                    .attribute("limit")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(50)
                    .min(50);
                let items = players
                    .into_iter()
                    .take(limit)
                    .map(|p| format!("<item id='{}' nickname='{}'/>", p.id, xml(&p.name)))
                    .collect::<String>();
                Ok(vec![result(
                    id,
                    &format!("<{tag} xmlns='{RESOLVER}'>{items}</{tag}>"),
                )])
            }
            HISTORY if kind == "get" => {
                anyhow::ensure!(
                    !id.is_empty() && id.len() <= 256,
                    "Invalid history request ID"
                );
                let peer = player_id(query.attribute("with").context("Missing history peer")?)?;
                let saved_messages = match self.shared.social.open(owner, peer).await {
                    Ok(room) => room.history,
                    Err(error) if error.is::<PrivateConversationBlocked>() => Vec::new(),
                    Err(error) => return Err(error),
                };
                let limit = query
                    .attribute("limit")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(100)
                    .min(100);
                let skip = saved_messages.len().saturating_sub(limit);
                let mut replies = vec![result(id, "")];
                let mut history = saved_messages.iter().skip(skip).peekable();
                while let Some(saved) = history.next() {
                    self.remember_message(saved.id);
                    replies.push(message(owner, saved, Some((id, history.peek().is_none()))));
                }
                if replies.len() == 1 {
                    replies.push(format!(
                        "<message type='chat' id='history-{}' from='{DOMAIN}' to='{}/{RESOURCE}'>{}</message>",
                        xml(id),
                        jid(owner),
                        history_marker(id, true),
                    ));
                }
                Ok(replies)
            }
            "http://wargaming.net/xmpp#storage" if kind != "get" => {
                Ok(vec![error(id, "feature-not-implemented")])
            }
            "http://wargaming.net/xmpp#storage" => Ok(vec![result(
                id,
                "<query xmlns='http://wargaming.net/xmpp#storage'><list name='contact-notes'/></query>",
            )]),
            "http://jabber.org/protocol/disco#items" => Ok(vec![result(
                id,
                "<query xmlns='http://jabber.org/protocol/disco#items'/>",
            )]),
            "http://jabber.org/protocol/disco#info" => Ok(vec![result(
                id,
                &format!(
                    "<query xmlns='{namespace}'><identity category='server' type='im' name='wot-server'/><feature var='jabber:iq:roster'/><feature var='urn:xmpp:blocking'/><feature var='{RESOLVER}'/><feature var='{HISTORY}'/></query>"
                ),
            )]),
            _ => Ok(vec![error(id, "feature-not-implemented")]),
        }
    }
}
