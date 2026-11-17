//! Protocol with reliability, defragmenting of bundles and channel support.

use crate::client_gateway::mercury::net::limits;
use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::num::NonZero;
use std::time::{Duration, Instant};

use tracing::{debug, instrument, trace, trace_span, warn};

use super::bundle::Bundle;
use super::packet::{Packet, PacketConfig, PacketConfigError, PacketLocked};
use super::seq::{Seq, SeqAlloc};

/// The (currently hardcoded) timeout on bundle fragments.
const FRAGMENT_TIMEOUT: Duration = Duration::from_secs(10);

/// A protocol tracker for an interface, providing support for accepting and preparing
/// bundles, with reliability, defragmenting and (off)channel support.
#[derive(Debug)]
pub struct Protocol {
    /// State shared with channel handles.
    shared: ProtocolShared,
    /// For each address we are exchanging with, we store a special off-channel channel.
    off_channels: HashMap<SocketAddr, OffChannel>,
    /// Known channels for each address, with optional channel indexing.
    channels: HashMap<(SocketAddr, Option<NonZero<u32>>), OnChannel>,
    maintenance_at: Instant,
    input_window: Instant,
    input_count: usize,
}

/// A structure referenced by any channel handle, containing shared states.
#[derive(Debug)]
struct ProtocolShared {
    /// Sequence number allocator used for off-channel communications.
    off_seq_alloc: SeqAlloc,
}

impl Protocol {
    pub fn new() -> Self {
        Self {
            shared: ProtocolShared {
                off_seq_alloc: SeqAlloc::new(Seq::ZERO + 1),
            },
            off_channels: HashMap::new(),
            channels: HashMap::new(),
            maintenance_at: Instant::now(),
            input_window: Instant::now(),
            input_count: 0,
        }
    }

    /// Drop channel state, pending reliable packets and fragment assemblies for a peer.
    pub fn remove_peer(&mut self, addr: SocketAddr) {
        self.off_channels.remove(&addr);
        self.channels.retain(|(peer, _), _| *peer != addr);
    }

    pub fn can_queue_reliable(&self, addr: SocketAddr, count: usize) -> bool {
        let total: usize = self
            .off_channels
            .values()
            .map(|channel| channel.off.out_reliable_packets.len())
            .chain(
                self.channels
                    .values()
                    .map(|channel| channel.off.out_reliable_packets.len()),
            )
            .sum();
        let current = self
            .channels
            .get(&(addr, None))
            .map_or(0, |channel| channel.off.out_reliable_packets.len());
        count <= limits::MAX_RELIABLE_OUTGOING.saturating_sub(current)
            && count <= limits::MAX_GLOBAL_RELIABLE_OUTGOING.saturating_sub(total)
    }

    pub fn transport_usage(&self) -> (usize, usize, usize) {
        let mut slots = 0;
        let mut packets = 0;
        for off in self
            .off_channels
            .values()
            .map(|channel| &channel.off)
            .chain(self.channels.values().map(|channel| &channel.off))
        {
            slots += off
                .in_fragments
                .values()
                .map(|fragments| fragments.fragments.len())
                .sum::<usize>();
            packets += off
                .in_fragments
                .values()
                .map(|fragments| fragments.seq_count as usize)
                .sum::<usize>();
            packets += off.in_bundles.iter().map(Bundle::len).sum::<usize>();
        }
        for channel in self.channels.values() {
            packets += channel.on.in_reliable_packets.len()
                + channel.on.in_reliable_contiguous_packets.len();
        }
        (
            self.off_channels.len() + self.channels.len(),
            slots,
            packets,
        )
    }

    pub fn maintenance(&mut self, now: Instant) {
        if now.saturating_duration_since(self.maintenance_at) < Duration::from_millis(100) {
            return;
        }
        self.maintenance_at = now;
        self.off_channels.retain(|_, channel| {
            channel.off.expire(now);
            now.saturating_duration_since(channel.off.last_received) < Duration::from_secs(120)
        });
        self.channels.retain(|_, channel| {
            channel.off.expire(now);
            now.saturating_duration_since(channel.off.last_received) < Duration::from_secs(120)
        });
    }

    /// Return the off-channel handle, the off-channel isn't a real channel and bundles
    /// prepared with it will be sent out of channel, but this is used as a shared
    /// interface with in-channel.
    pub fn off_channel(&mut self, addr: SocketAddr) -> Channel<'_> {
        let channel = self.off_channels.entry(addr).or_insert_with(|| OffChannel {
            off: OffChannelData::new(),
        });

        Channel {
            inner: GenericChannel {
                shared: &mut self.shared,
                off: &mut channel.off,
                on: None,
            },
        }
    }

    /// Return a handle to a channel associated with the given address, optionally
    /// indexed if desired, the channel is created if not already existing with initial
    /// version of 1.
    pub fn channel(&mut self, addr: SocketAddr, index: Option<NonZero<u32>>) -> Channel<'_> {
        let channel = self
            .channels
            .entry((addr, index))
            .or_insert_with(|| OnChannel {
                off: OffChannelData::new(),
                on: match index {
                    None => OnChannelData::new_without_index(),
                    Some(index) => OnChannelData::new_with_index(index),
                },
            });

        Channel {
            inner: GenericChannel {
                shared: &mut self.shared,
                off: &mut channel.off,
                on: Some(&mut channel.on),
            },
        }
    }

    /// The reliable packets sent at least `timeout` ago and still not acknowledged, to
    /// send again, with their address: a lost one would otherwise hold back every later
    /// one at the other end. Each is then due again after another `timeout`, and dropped
    /// once unacknowledged for `give_up`, its peer most likely gone.
    pub fn take_resends(
        &mut self,
        timeout: Duration,
        give_up: Duration,
    ) -> Vec<(SocketAddr, Packet)> {
        let now = Instant::now();
        let offs = self
            .off_channels
            .iter_mut()
            .map(|(&addr, channel)| (addr, &mut channel.off));
        let ons = self
            .channels
            .iter_mut()
            .map(|(&(addr, _), channel)| (addr, &mut channel.off));
        let mut resends = Vec::new();
        for (addr, off) in offs.chain(ons) {
            off.out_reliable_packets.retain(|packet| {
                let keep = now.duration_since(packet.time) < give_up;
                if !keep {
                    warn!(%addr, "Gave up resending reliable packet #{}, never acknowledged", packet.sequence_num);
                }
                keep
            });
            for packet in &mut off.out_reliable_packets {
                if now.duration_since(packet.sent) >= timeout {
                    debug!(%addr, "Resending reliable packet #{}", packet.sequence_num);
                    packet.sent = now;
                    resends.push((addr, packet.packet.clone()));
                }
            }
        }
        resends
    }

    /// Accept a new incoming packet and return the associated channel where the packet
    /// has been accepted. If any error in packet's decoding happens, then it's returned
    /// as an error and this packet, this packet cannot be accepted as-is. Any piggyback
    /// packet is still registered but errors are not forwarded.
    #[instrument(name = "accept", level = "trace", skip(self, packet))]
    #[inline(always)]
    pub fn accept(&mut self, packet: Packet, addr: SocketAddr) -> Result<Channel<'_>, Packet> {
        self.accept_inner(packet, addr, 0)
    }

    /// Internal wrapper used to improve tracing of recursive span with piggybacks.
    fn accept_inner(
        &mut self,
        packet: Packet,
        addr: SocketAddr,
        depth: usize,
    ) -> Result<Channel<'_>, Packet> {
        if depth >= limits::MAX_PIGGYBACK_DEPTH {
            return Err(packet);
        }
        let time = Instant::now();
        self.maintenance(time);
        if time.saturating_duration_since(self.input_window) >= Duration::from_secs(1) {
            self.input_window = time;
            self.input_count = 0;
        }
        if self.input_count >= limits::MAX_PACKETS_PER_SECOND {
            return Err(packet);
        }
        self.input_count += 1;
        let mut packet = match packet.read_config_locked() {
            Ok(packet) => packet,
            Err((error, packet)) => {
                warn!("Failed to read config: {error}");
                return Err(packet);
            }
        };

        // Immediately process any piggyback packet, because they must have been
        // initially sent way before the current packet we are decoding.
        for piggyback in std::mem::take(packet.piggybacks_mut()) {
            trace!(bytes = piggyback.len(), "Processing piggyback packet");
            let _span = trace_span!("pigb").entered();
            // Ignore any error on channel decoding.
            let _ = self.accept_inner(piggyback, addr, depth + 1);
        }

        let index = packet.config().indexed_channel().map(|(index, _)| index);
        let off = if packet.config().on_channel() {
            self.channels
                .get(&(addr, index))
                .map(|channel| &channel.off)
        } else {
            self.off_channels.get(&addr).map(|channel| &channel.off)
        };
        let (_, slots, buffered) = self.transport_usage();
        if buffered >= limits::MAX_BUFFERED_PACKETS {
            return Err(packet.destruct().0);
        }
        if let Some((first, last)) = packet.config().sequence_range() {
            let existing = off.is_some_and(|off| off.in_fragments.contains_key(&first));
            if !existing && slots + (last - first + 1) as usize > limits::MAX_FRAGMENT_SLOTS {
                return Err(packet.destruct().0);
            }
        }
        let known = if packet.config().on_channel() {
            self.channels.contains_key(&(addr, index))
        } else {
            self.off_channels.contains_key(&addr)
        };
        if !known && self.channels.len() + self.off_channels.len() >= limits::MAX_PEER_STATES {
            return Err(packet.destruct().0);
        }

        // Start by finding the appropriate channel for this packet regarding the local
        // socket address and channel-related flags on this packet.
        let mut channel;
        if packet.config().on_channel() {
            let on_channel;
            if let Some((index, version)) = packet.config().indexed_channel() {
                trace!("Is on-channel: {index} v{version}");
                on_channel =
                    self.channels
                        .entry((addr, Some(index)))
                        .or_insert_with(|| OnChannel {
                            off: OffChannelData::new(),
                            on: OnChannelData::new_with_index_version(index, version),
                        });

                // Unwrap because the channel should have index.
                let current_version = on_channel.on.index.unwrap().version;
                if version < current_version {
                    trace!("Outdated, expected v{current_version}");
                    // TODO: outdated packet
                    return Err(packet.destruct().0);
                }
            } else {
                trace!("Is on-channel: not indexed");
                on_channel = self
                    .channels
                    .entry((addr, None))
                    .or_insert_with(|| OnChannel {
                        off: OffChannelData::new(),
                        on: OnChannelData::new_without_index(),
                    });
            }

            channel = GenericChannel {
                shared: &mut self.shared,
                off: &mut on_channel.off,
                on: Some(&mut on_channel.on),
            };
        } else {
            trace!("Is off-channel");
            let off_channel = self.off_channels.entry(addr).or_insert_with(|| OffChannel {
                off: OffChannelData::new(),
            });

            channel = GenericChannel {
                shared: &mut self.shared,
                off: &mut off_channel.off,
                on: None,
            };
        }

        channel.off.last_received = time;
        if channel.off.in_bundles.len() >= limits::MAX_QUEUED_BUNDLES
            || channel.off.in_reliable_packets.len() >= limits::MAX_CHANNEL_BUFFERED_PACKETS
        {
            return Err(packet.destruct().0);
        }
        if let Some(on) = channel.on.as_deref_mut()
            && on.in_reliable_packets.len() + on.in_reliable_contiguous_packets.len()
                >= limits::MAX_CHANNEL_BUFFERED_PACKETS
        {
            return Err(packet.destruct().0);
        }

        // Cumulative ack is not supposed to be used off-channel.
        if let Some(cumulative_ack) = packet.config().cumulative_ack() {
            // Cumulative ack is not supported off-channel.
            if channel.on.is_none() {
                warn!("Cumulative ack is not supported off-channel");
                return Err(packet.destruct().0);
            }

            channel
                .off
                .ack_out_reliable_packet_cumulative(cumulative_ack);
        }

        // Immediately handle single acks because this packet may be buffered to be
        // processed in-order.
        for &ack in packet.config().single_acks() {
            channel.off.ack_out_reliable_packet(ack);
        }

        // Reliable packet must be acknowledged later.
        if packet.config().reliable() {
            if packet.config().last_reliable_sequence_num().is_some() {
                warn!("Last reliable sequence is not support with reliable");
                return Err(packet.destruct().0);
            }

            let sequence_num = packet.config().sequence_num();
            channel.off.add_in_reliable_packet(sequence_num);

            // When on-channel with reliable packets, we must track the cumulative ack
            // and buffer any packet that is received out-of-order!
            if let Some(on) = channel.on.as_deref_mut() {
                on.add_in_reliable_packet(packet);
                while channel.off.in_bundles.len() < limits::MAX_QUEUED_BUNDLES {
                    let Some(bundle) = on.pop_in_reliable_bundle() else {
                        break;
                    };
                    channel.off.in_bundles.push_back(bundle);
                }
                // Shortcut to
                return Ok(Channel { inner: channel });
            }
        } else if let Some(last_reliable_sequence_num) =
            packet.config().last_reliable_sequence_num()
        {
            // In this case we must ensure that current expected sequence is equal to
            // this given sequence number + 1.
            if let Some(on) = channel.on.as_deref_mut() {
                if last_reliable_sequence_num != on.in_reliable_expected_seq - 1 {
                    warn!(
                        "Invalid last reliable sequence number, expected: {}, got: {}",
                        on.in_reliable_expected_seq - 1,
                        last_reliable_sequence_num
                    );
                    return Err(packet.destruct().0);
                }
            } else {
                warn!("Last reliable sequence is not supported off-channel");
                return Err(packet.destruct().0);
            }
        }

        // If we land here, it's either because the packet isn't reliable, or if the
        // packet is reliable but we are not in-channel (the latter seems forbidden by
        // WG source code, but it must be verified). TLDR, the packet don't need to
        // be reordered, so the logic is much simpler: we use off-channel fragments map.
        channel.off.add_in_packet(packet, time);

        Ok(Channel { inner: channel })
    }
}

impl Default for Protocol {
    fn default() -> Self {
        Self::new()
    }
}

/// A handle to a channel or to the special off-channel fake channel.
///
/// Represent a channel between the app and a client with specific socket address. A
/// channel is a way to create multiple communication channels between the same pair
/// of addresses and also provides support for reliable communication with sequence
/// number acks and resend logic (not yet implemented).
#[derive(Debug)]
pub struct Channel<'a> {
    inner: GenericChannel<'a>,
}

impl Channel<'_> {
    /// Return true if this generic channel represents an actual on-channel one.
    pub fn is_on(&self) -> bool {
        self.inner.on.is_some()
    }

    /// Return true if this generic channel represents an actual off-channel one.
    pub fn is_off(&self) -> bool {
        !self.is_on()
    }

    /// If this generic channel is an actual on-channel one (see [`Self::is_on`]), and
    /// it has and index (and version) then it's returned.
    pub fn index(&self) -> Option<ChannelIndex> {
        self.inner.on.as_deref().and_then(|on| on.index)
    }

    /// Pop the next bundle able to be received, if any, this ensures that bundles are
    /// received in the correct order!
    pub fn next_bundle(&mut self) -> Option<Bundle> {
        self.inner.off.in_bundles.pop_front()
    }

    /// Pop all the next bundles, in order, and without borrowing the channel, returning
    /// an owned iterator. In general, it's better to use the [`Self::next_bundle`]
    /// function when borrowing is not an issue.
    ///
    /// This function does not allocate if there is only one (or zero) bundle!
    pub fn pop_bundles(&mut self) -> impl Iterator<Item = Bundle> + use<> {
        // Our goal is to avoid allocation when we only have one bundle, so we use a vec
        // only when necessary and return the chained option + vec.
        let bundle = self.inner.off.in_bundles.pop_front();
        // Collecting no bundle should not allocate any vector's memory.
        let rest = self.inner.off.in_bundles.drain(..).collect::<Vec<_>>();

        bundle.into_iter().chain(rest)
    }

    /// Prepare a bundle to be sent, adding acks and other configuration required by this
    /// tracker into all packets. After this function, all packets are ready to be sent
    /// and the bundle should not be touched for this to remain true.
    ///
    /// NOTE: FIXME: It's said that external interfaces don't allow off-channel reliable
    /// communication (see packet_receiver.cpp, line 977).
    #[instrument(level = "trace", skip(self, bundle))]
    pub fn prepare(&mut self, bundle: &mut Bundle, reliable: bool) {
        let time = Instant::now();
        let bundle_len = bundle.len() as u32;
        trace!("Count: {bundle_len}");

        // Create a common packet config for all the bundle.
        let mut packet_config = PacketConfig::new();

        packet_config.set_reliable(reliable);

        let mut sequence_num = None;
        if bundle_len > 1 || reliable {
            let first = self.inner.alloc_sequence_num(bundle_len, reliable);
            trace!(
                "Allocated sequence numbers: {}..{}",
                first,
                first + bundle_len
            );
            packet_config.set_sequence_num(first);
            sequence_num = Some(first);
        }

        if let Some(on) = self.inner.on.as_deref_mut() {
            packet_config.set_on_channel(true);
            packet_config.set_cumulative_ack(on.in_reliable_expected_seq);
            if let Some(index) = on.index {
                packet_config.set_indexed_channel(index.index, index.version);
                trace!("Is on-channel: {} v{}", index.index, index.version);
            } else {
                trace!("Is on-channel: not indexed");
            }
        } else {
            trace!("Is off-channel");
        }

        if !self.inner.off.in_reliable_packets.is_empty() {
            trace!(
                "Pending single acks: {:?}",
                self.inner.off.in_reliable_packets
            );
        }

        trace!("Using prefix offset: 0x{:08X}", 0);

        // This swap is simple: it places the dequeue of all received reliable packets
        // and their sequence numbers into the packet config's acks queue. We must
        // remember after this to transfer back the remaining sequence numbers that
        // have not been sent from the packet config.
        std::mem::swap(
            &mut self.inner.off.in_reliable_packets,
            packet_config.single_acks_mut(),
        );
        debug_assert!(
            self.inner.off.in_reliable_packets.is_empty(),
            "packet config acks were not empty"
        );

        bundle.write_config(&mut packet_config);
        bundle.update_prefix(0);

        // Now we need to restore acks that have not been sent: swap back (read above).
        std::mem::swap(
            &mut self.inner.off.in_reliable_packets,
            packet_config.single_acks_mut(),
        );
        debug_assert!(
            packet_config.single_acks().is_empty(),
            "packet config acks should be empty"
        );

        // Kept as finished, to be sent again until acknowledged.
        if let (true, Some(first)) = (reliable, sequence_num) {
            for (i, packet) in bundle.iter().enumerate() {
                self.inner
                    .off
                    .add_out_reliable_packet(first + i as u32, time, packet.clone());
            }
        }

        if !self.inner.off.in_reliable_packets.is_empty() {
            trace!(
                "Remaining single acks: {:?}",
                self.inner.off.in_reliable_packets
            )
        }
    }
}

/// Documented from BigWorld source code (programming\bigworld\lib\network\udp_channel.hpp).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelIndex {
    /// An indexed channel is basically a way of multiplexing multiple
    /// channels between a pair of addresses.  Regular channels distinguish
    /// traffic solely on the basis of address, so in situations where you need
    /// multiple channels between a pair of addresses (i.e. channels between
    /// base and cell entities) you use indexed channels to keep the streams
    /// separate.
    pub index: NonZero<u32>,
    /// Indexed channels have a 'version' number which basically tracks how
    /// many times they have been offloaded.  This allows us to correctly
    /// determine which incoming packets are out-of-date and also helps
    /// identify the most up-to-date information about lost entities in a
    /// restore situation.
    pub version: NonZero<u32>,
}

///  Kind of error that caused a packet to be rejected from this socket and not received.
#[derive(Debug, Clone, thiserror::Error)]
pub enum PacketRejectionError {
    /// The packet is part of a sequence but no other packets of the sequence have been
    /// found and therefore no bundle can be reconstructed.
    #[error("timed out")]
    TimedOut,
    /// The packet could not be synchronized from its data.
    #[error("config error: {0}")]
    Config(#[from] PacketConfigError),
}

/// Common data for off-channel and on-channel communication.
#[derive(Debug)]
struct OffChannelData {
    /// All sequences marked as reliable, so that we can ensure that they are received.
    /// It should be naturally sorted (debug_asserted).
    out_reliable_packets: Vec<OutReliablePacket>,
    /// A dequeue containing all received reliable packets for which we should send ack.
    /// It doesn't need to be sorted.
    in_reliable_packets: VecDeque<Seq>,
    /// Bundle fragments tracking, mapped to the first sequence.
    in_fragments: HashMap<Seq, Fragments>,
    /// Buffered bundles that can be retrieved by the client!
    in_bundles: VecDeque<Bundle>,
    last_received: Instant,
}

/// A reliable packet that we sent at given time and waiting for an acknowledgment.
#[derive(Debug)]
struct OutReliablePacket {
    /// The sequence number.
    sequence_num: Seq,
    /// The time this sequence has been sent.
    time: Instant,
    /// The packet as sent, retained until acknowledged or expired.
    packet: Packet,
    /// When it was last sent.
    sent: Instant,
}

impl OffChannelData {
    fn new() -> Self {
        Self {
            out_reliable_packets: Vec::new(),
            in_reliable_packets: VecDeque::new(),
            in_fragments: HashMap::new(),
            in_bundles: VecDeque::new(),
            last_received: Instant::now(),
        }
    }

    fn expire(&mut self, now: Instant) {
        self.in_fragments
            .retain(|_, fragments| !fragments.is_old(now, FRAGMENT_TIMEOUT));
        self.out_reliable_packets
            .retain(|packet| now.saturating_duration_since(packet.time) < Duration::from_secs(30));
    }

    /// Register a reliable packet just sent, see [`Protocol::take_resends`].
    fn add_out_reliable_packet(&mut self, sequence_num: Seq, time: Instant, packet: Packet) {
        // We are keeping reliable packets ordered by their sequence number and also by
        // their time (Instant::now() can only grow).
        debug_assert!(
            self.out_reliable_packets.is_empty()
                || self
                    .out_reliable_packets
                    .last()
                    .unwrap()
                    .sequence_num
                    .wrapping_cmp(sequence_num)
                    .is_le(),
            "reliable packet sequence number should be greater than previous ones"
        );

        trace!("Add reliable packet: {sequence_num}");
        if self.out_reliable_packets.len() >= limits::MAX_RELIABLE_OUTGOING {
            return;
        }
        self.out_reliable_packets.push(OutReliablePacket {
            sequence_num,
            time,
            packet,
            sent: time,
        });
    }

    /// When a single ack is received on a packet, this can be called to
    fn ack_out_reliable_packet(&mut self, sequence_num: Seq) {
        // Naive search for now, because Seq is complicated!
        let index = self
            .out_reliable_packets
            .iter()
            .position(|packet| sequence_num == packet.sequence_num);

        if let Some(index) = index {
            let reliable_packet = self.out_reliable_packets.remove(index);
            trace!(
                "Single ack for reliable packet: {sequence_num} after {:?}",
                reliable_packet.time.elapsed()
            );
        }
    }

    /// When a cumulative ack is received, this can be used to acknowledge all sequences
    /// up to, but excluding, the given sequence number. Not supported off-channel but
    /// still present here.
    fn ack_out_reliable_packet_cumulative(&mut self, sequence_num: Seq) {
        // Naive search for now, because Seq is complicated!
        // Using '.le' because it's cumulative ack is exclusive!
        let drain_len = self
            .out_reliable_packets
            .iter()
            .position(|packet| sequence_num.wrapping_cmp(packet.sequence_num).is_le())
            .unwrap_or(self.out_reliable_packets.len());

        debug!("Cumulative ack for reliable packets: ..{sequence_num}");
        for reliable_packet in self.out_reliable_packets.drain(..drain_len) {
            trace!(
                "Cumulative ack for a previous packet: {}, after: {:?}",
                reliable_packet.sequence_num,
                reliable_packet.time.elapsed()
            );
        }
    }

    /// Register a simple reliable packet to be acknowledged in the future.
    fn add_in_reliable_packet(&mut self, sequence_num: Seq) {
        // It's unsorted so we don't care of duplicate entries!
        self.in_reliable_packets.push_back(sequence_num);
        trace!("Received reliable packet: {sequence_num}");
    }

    /// Push a packet that may be a bundle's fragment, if a bundle is completed, it is
    /// added to the internal bundles queue, there is no ordering guaranteed with such
    /// packet, see [`OnChannelData::add_in_reliable_packet`] for reordering.
    fn add_in_packet(&mut self, packet: PacketLocked, time: Instant) {
        self.expire(time);
        let bundle = match packet.config().sequence_range() {
            Some((first_seq, last_seq)) => {
                let count = last_seq - first_seq + 1;
                let index = packet.config().sequence_num() - first_seq;
                if count > limits::MAX_BUNDLE_PACKETS as u32 || index >= count {
                    return;
                }
                if !self.in_fragments.contains_key(&first_seq)
                    && self.in_fragments.len() >= limits::MAX_FRAGMENT_ASSEMBLIES
                {
                    return;
                }
                let fragments = self
                    .in_fragments
                    .entry(first_seq)
                    .or_insert_with(|| Fragments::new(count));
                if fragments.fragments.len() != count as usize {
                    return;
                }
                fragments.set(index, packet);
                if !fragments.is_full() {
                    return;
                }
                let Some(fragments) = self.in_fragments.remove(&first_seq) else {
                    return;
                };
                fragments.into_bundle()
            }
            _ => Bundle::new_with_single(packet),
        };

        self.in_bundles.push_back(bundle);
    }
}

/// Data specific to on-channel communication.
#[derive(Debug)]
struct OnChannelData {
    /// Optional index for this channel.
    index: Option<ChannelIndex>,
    /// The next sequence number to return for this channel, **only for reliable
    /// packets**, non-reliable packets sent on-channel are using the off-channel alloc.
    seq_alloc: SeqAlloc,
    /// Most of the time, reliable sequence numbers are received in order, and so we can
    /// just increment this counter in order to know that all packets up to (but
    /// excluding) this number has been received.
    ///
    /// NOTE: This is the same as `inSeqAt_` in BW source.
    in_reliable_expected_seq: Seq,
    in_reliable_contiguous_packets: VecDeque<PacketLocked>,
    in_reliable_packets: VecDeque<PacketLocked>,
}

impl OnChannelData {
    fn new(index: Option<ChannelIndex>) -> Self {
        Self {
            index,
            seq_alloc: SeqAlloc::new(Seq::ZERO),
            in_reliable_expected_seq: Seq::ZERO,
            in_reliable_contiguous_packets: VecDeque::new(),
            in_reliable_packets: VecDeque::new(),
        }
    }

    fn new_without_index() -> Self {
        Self::new(None)
    }

    fn new_with_index_version(index: NonZero<u32>, version: NonZero<u32>) -> Self {
        Self::new(Some(ChannelIndex { index, version }))
    }

    fn new_with_index(index: NonZero<u32>) -> Self {
        Self::new_with_index_version(index, NonZero::new(1).unwrap())
    }

    /// Add a received (in) reliable packet to the internal re-ordering logic of this
    /// channel, this will automatically construct an ordered bundle when completed.
    /// This also updates the cumulative ack that can be sent back.
    ///
    /// After this function has filled contiguous and buffered packets, you may want to
    /// user [`Self::pop_in_reliable_bundle`] to pop any completed contiguous bundle.
    ///
    /// Returns `true` if this sequence number had already been fully processed or was
    /// already buffered — i.e. this call carried no new information. This commonly
    /// happens when a peer piggybacks a still-unacknowledged reliable packet onto a
    /// later, unrelated one: from here it looks identical to a plain retransmission.
    fn add_in_reliable_packet(&mut self, packet: PacketLocked) -> bool {
        debug_assert!(
            packet.config().reliable(),
            "given packet should be reliable"
        );

        let sequence_num = packet.config().sequence_num();

        match sequence_num.wrapping_cmp(self.in_reliable_expected_seq) {
            Ordering::Equal => {
                // This is the best scenario, packet is received in-order, so we push the
                // packet after the currently contiguous sequence.
                self.in_reliable_expected_seq += 1;
                self.in_reliable_contiguous_packets.push_back(packet);

                // By inserting this packet, we may a filled a gap in the sequences, so
                // we increment it while it's possible.
                while let Some(packet) = self.in_reliable_packets.front() {
                    if packet.config().sequence_num() == self.in_reliable_expected_seq {
                        trace!(
                            "Unbuffered reliable packet: {}",
                            packet.config().sequence_num()
                        );
                        self.in_reliable_expected_seq += 1;
                        self.in_reliable_contiguous_packets
                            .push_back(self.in_reliable_packets.pop_front().unwrap());
                    } else {
                        break; // Not contiguous.
                    }
                }
            }
            Ordering::Less => {
                // Do nothing, the sequence number may have been already received...
                return true;
            }
            Ordering::Greater => {
                // Warning if we get many buffered packets which indicate that we probably
                // lost track of one of the
                if self.in_reliable_packets.len() > 50 {
                    warn!(
                        "Buffered too many in reliable packets: {}",
                        self.in_reliable_packets.len()
                    );
                }

                // We search where we can insert the packet, starting from the end because
                // it's still likely to receive packets in order.
                let mut insert_index = 0;
                for (i, buffered_packet) in self.in_reliable_packets.iter().enumerate().rev() {
                    match sequence_num.wrapping_cmp(buffered_packet.config().sequence_num()) {
                        Ordering::Equal => return true, // Duplicate packet, just abort.
                        Ordering::Less => continue,
                        Ordering::Greater => {
                            insert_index = i + 1;
                            break;
                        }
                    }
                }

                self.in_reliable_packets.insert(insert_index, packet);
                trace!("Buffered reliable packet at: {insert_index}");

                // let debug_seqs = self.in_reliable_packets.iter()
                //     .map(|packet| packet.config().sequence_num().get())
                //     .collect::<Vec<_>>();
                // trace!("Buffered packets: {debug_seqs:?}");
            }
        }

        trace!(
            "Received reliable packet cumulative: {}, contiguous: {}, buffered: {} (first: {:?})",
            self.in_reliable_expected_seq,
            self.in_reliable_contiguous_packets.len(),
            self.in_reliable_packets.len(),
            self.in_reliable_packets
                .front()
                .map(|packet| packet.config().sequence_num().get())
        );

        false
    }

    /// Try to construct any reliable bundle if possible.
    fn pop_in_reliable_bundle(&mut self) -> Option<Bundle> {
        loop {
            let first_packet = self.in_reliable_contiguous_packets.front()?;
            if let Some((first_seq, last_seq)) = first_packet.config().sequence_range() {
                trace!(
                    "Reliable fragment: {}..={}",
                    first_seq.get(),
                    last_seq.get()
                );

                // Checking coherency of the sequence range, the first contiguous packet
                // should also start the range, if not the case we just pop that invalid
                // packet.
                if first_packet.config().sequence_num() != first_seq {
                    warn!(
                        "Missing the first fragment packet, got: {}, range: {}..={}",
                        first_packet.config().sequence_num(),
                        first_seq.get(),
                        last_seq.get()
                    );
                    // Forget this invalid packet and continue to the next packet.
                    let _ = self.in_reliable_contiguous_packets.pop_front();
                    continue;
                }

                // Because we ensure that packets are contiguous with strictly increasing
                // seq, we just check that we have enough packets to complete this bundle,
                // this guarantee that we have all first_seq..=last_seq existing in it.
                // TODO: Add a anti-DOS check here to ensure that bundle length isn't absurd.
                let bundle_len = (last_seq - first_seq + 1) as usize;
                if self.in_reliable_contiguous_packets.len() < bundle_len {
                    trace!(
                        "Reliable fragment: not enough contiguous, expected: {}, got: {}",
                        bundle_len,
                        self.in_reliable_contiguous_packets.len()
                    );
                    return None;
                }

                // Enough packets, check that all packets have the same sequence range.
                let mut coherent = true;
                for second_packet in self
                    .in_reliable_contiguous_packets
                    .iter()
                    .take(bundle_len)
                    .skip(1)
                {
                    if second_packet.config().sequence_range() != Some((first_seq, last_seq)) {
                        debug!(
                            range = ?second_packet.config().sequence_range(),
                            expected = ?(first_seq, last_seq),
                            "Incoherent reliable fragment",
                        );
                        coherent = false;
                        break;
                    }
                }

                // We drain all elements anyway, but only returns if all packets are valid.
                let drain = self.in_reliable_contiguous_packets.drain(..bundle_len);
                if coherent {
                    return Some(Bundle::new_with_multiple(drain));
                } else {
                    continue;
                }
            } else {
                // Unwrap because we know that this packet exists.
                let single_packet = self.in_reliable_contiguous_packets.pop_front().unwrap();
                return Some(Bundle::new_with_single(single_packet));
            }
        }
    }
}

#[derive(Debug)]
#[repr(C)]
struct OffChannel {
    off: OffChannelData,
}

#[derive(Debug)]
#[repr(C)]
struct OnChannel {
    off: OffChannelData,
    on: OnChannelData,
}

/// Internal structure used to reference a channel like a handle to it, providing an
/// internal common interface between both.
#[derive(Debug)]
struct GenericChannel<'a> {
    shared: &'a mut ProtocolShared,
    off: &'a mut OffChannelData,
    on: Option<&'a mut OnChannelData>,
}

impl GenericChannel<'_> {
    /// Generic sequence number allocation depending on the context of the channel.
    fn alloc_sequence_num(&mut self, count: u32, reliable: bool) -> Seq {
        if let Some(on) = self.on.as_deref_mut()
            && reliable
        {
            return on.seq_alloc.alloc(count);
        }
        self.shared.off_seq_alloc.alloc(count)
    }
}

/// Internal structure to keep fragments from a given sequence.
#[derive(Debug)]
struct Fragments {
    fragments: Vec<Option<PacketLocked>>,
    seq_count: u32,
    last_update: Instant,
}

impl Fragments {
    /// Create from sequence length.
    fn new(seq_len: u32) -> Self {
        Self {
            fragments: (0..seq_len).map(|_| None).collect(),
            seq_count: 0,
            last_update: Instant::now(),
        }
    }

    /// Set a fragment.
    fn set(&mut self, num: u32, packet: PacketLocked) {
        let Some(frag) = self.fragments.get_mut(num as usize) else {
            return;
        };
        if frag.is_none() {
            self.seq_count += 1;
        }
        self.last_update = Instant::now();
        *frag = Some(packet);
    }

    #[inline]
    fn is_old(&self, time: Instant, timeout: Duration) -> bool {
        time.saturating_duration_since(self.last_update) > timeout
    }

    #[inline]
    fn is_full(&self) -> bool {
        self.seq_count as usize == self.fragments.len()
    }

    /// Convert this structure to a bundle, **safe to call only if `is_full() == true`**.
    #[inline]
    fn into_bundle(self) -> Bundle {
        assert!(self.is_full());
        self.fragments.into_iter().map(|o| o.unwrap()).collect()
    }
}

#[cfg(test)]
mod tests {

    use std::net::{Ipv4Addr, SocketAddr};

    use super::*;
    use crate::client_gateway::mercury::net::element::DebugElementFixed;

    /// The sequence number the next reliable packet on the channel would get.
    fn next_reliable(protocol: &mut Protocol, addr: SocketAddr) -> Seq {
        protocol
            .channel(addr, None)
            .inner
            .alloc_sequence_num(1, true)
    }

    /// The other end's ten reliable packets, accepted by `protocol`.
    fn accept_ten(protocol: &mut Protocol, addr: SocketAddr) {
        let mut peer = Protocol::new();
        for _ in 0..10 {
            let mut bundle = Bundle::new();
            bundle
                .element_writer()
                .write_simple(DebugElementFixed::<0x10, 1> { data: [0] })
                .unwrap();
            peer.channel(addr, None).prepare(&mut bundle, true);
            for packet in bundle.iter() {
                assert!(protocol.accept(packet.clone(), addr).is_ok());
            }
        }
    }

    /// What an endpoint receives doesn't number what it sends: the other end would wait
    /// forever for the numbers skipped.
    #[test]
    fn endpoint_numbers_independently() {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 1));
        let first = next_reliable(&mut Protocol::new(), addr);
        let mut server = Protocol::new();
        accept_ten(&mut server, addr);
        assert_eq!(next_reliable(&mut server, addr), first);
    }
}
