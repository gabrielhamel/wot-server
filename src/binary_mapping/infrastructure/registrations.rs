use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use iced_x86::{Decoder, DecoderOptions, FlowControl, Instruction, Mnemonic, OpKind, Register};

use super::pe::Image;
use crate::binary_mapping::domain::{Framing, Interface, Message};

#[derive(Clone, Debug)]
struct Call {
    address: u64,
    target: u64,
    args: [Option<u64>; 4],
    handler: Option<u64>,
    argument_loads: [Option<u64>; 4],
    return_store: Option<u64>,
}

fn immediate(instruction: &Instruction, operand: u32) -> Option<u64> {
    match instruction.op_kind(operand) {
        OpKind::Immediate8
        | OpKind::Immediate16
        | OpKind::Immediate32
        | OpKind::Immediate64
        | OpKind::Immediate8to16
        | OpKind::Immediate8to32
        | OpKind::Immediate8to64
        | OpKind::Immediate32to64 => Some(instruction.immediate(operand)),
        _ => None,
    }
}

fn trace(image: &Image<'_>, start: u64, end: u64) -> Vec<Call> {
    let Some(bytes) = image.slice(start, (end - start) as usize) else {
        return Vec::new();
    };
    let mut registers = BTreeMap::<Register, u64>::new();
    let mut handler = None;
    let mut loads = BTreeMap::<Register, u64>::new();
    let mut calls: Vec<Call> = Vec::new();
    let mut decoder = Decoder::with_ip(64, bytes, start, DecoderOptions::NONE);
    while decoder.can_decode() {
        let instruction = decoder.decode();
        if instruction.is_invalid() {
            break;
        }
        if instruction.mnemonic() == Mnemonic::Call {
            if instruction.op0_kind() == OpKind::NearBranch64 {
                calls.push(Call {
                    address: instruction.ip(),
                    target: instruction.near_branch_target(),
                    args: [Register::RCX, Register::RDX, Register::R8, Register::R9]
                        .map(|r| registers.get(&r).copied()),
                    handler,
                    argument_loads: [Register::RCX, Register::RDX, Register::R8, Register::R9]
                        .map(|r| loads.get(&r).copied()),
                    return_store: None,
                });
            }
            for register in [
                Register::RAX,
                Register::RCX,
                Register::RDX,
                Register::R8,
                Register::R9,
                Register::R10,
                Register::R11,
            ] {
                registers.remove(&register);
                loads.remove(&register);
            }
            handler = None;
            continue;
        }
        if instruction.flow_control() != FlowControl::Next {
            break;
        }
        if instruction.mnemonic() == Mnemonic::Mov
            && instruction.op0_kind() == OpKind::Memory
            && instruction.is_ip_rel_memory_operand()
            && instruction.op1_register() == Register::RAX
            && let Some(call) = calls.last_mut()
        {
            call.return_store = Some(instruction.ip_rel_memory_address());
        }
        let value = match instruction.mnemonic() {
            Mnemonic::Lea if instruction.is_ip_rel_memory_operand() => {
                Some(instruction.ip_rel_memory_address())
            }
            Mnemonic::Mov => immediate(&instruction, 1).or_else(|| {
                (instruction.op1_kind() == OpKind::Register)
                    .then(|| {
                        registers
                            .get(&instruction.op1_register().full_register())
                            .copied()
                    })
                    .flatten()
            }),
            Mnemonic::Xor if instruction.op0_register() == instruction.op1_register() => Some(0),
            _ => None,
        };
        if instruction.op0_kind() == OpKind::Register {
            let register = instruction.op0_register().full_register();
            registers.remove(&register);
            loads.remove(&register);
            if instruction.mnemonic() == Mnemonic::Mov
                && instruction.op1_kind() == OpKind::Memory
                && instruction.is_ip_rel_memory_operand()
            {
                loads.insert(register, instruction.ip_rel_memory_address());
            }
            if let Some(value) = value {
                registers.insert(register, value);
            }
        }
        if instruction.mnemonic() == Mnemonic::Mov
            && instruction.op0_kind() == OpKind::Memory
            && instruction.memory_base() == Register::RSP
            && instruction.memory_displacement64() == 0x20
        {
            handler = value;
        }
    }
    calls
}

pub fn extract(image: &Image<'_>) -> Result<(u64, Vec<Interface>)> {
    let mut callers = BTreeMap::new();
    let mut anchors = BTreeMap::<String, BTreeSet<u64>>::new();
    for &(start, end) in &image.functions {
        if end - start > 512 {
            continue;
        }
        let calls = trace(image, start, end);
        for call in &calls {
            if let Some(name) = call.args[1].and_then(|address| image.string(address))
                && ["createBasePlayer", "createCellPlayer"].contains(&name.as_str())
            {
                anchors.entry(name).or_default().insert(call.target);
            }
        }
        if !calls.is_empty() {
            callers.insert(start, calls);
        }
    }
    let base = anchors.get("createBasePlayer").context(
        "could not locate createBasePlayer registration; unsupported client registration pattern",
    )?;
    let cell = anchors
        .get("createCellPlayer")
        .context("could not locate createCellPlayer registration")?;
    let shared: Vec<_> = base.intersection(cell).copied().collect();
    ensure!(
        shared.len() == 1,
        "ambiguous InterfaceMinder::add discovery"
    );
    let target = shared[0];
    let result_slots: BTreeSet<_> = callers
        .values()
        .flatten()
        .filter(|c| c.target == target)
        .filter_map(|c| c.return_store)
        .collect();
    let range_targets: BTreeSet<_> = callers
        .values()
        .flatten()
        .filter(|c| {
            c.argument_loads[1].is_some_and(|v| result_slots.contains(&v))
                && c.args[0].is_some()
                && c.args[2].is_some()
        })
        .map(|c| c.target)
        .collect();
    ensure!(
        range_targets.len() <= 1,
        "ambiguous InterfaceMinder::addRange discovery"
    );
    let range_target = range_targets.first().copied();
    callers.retain(|_, calls| {
        calls
            .iter()
            .any(|call| call.target == target || Some(call.target) == range_target)
    });
    let mut slots = BTreeMap::<u64, Vec<u64>>::new();
    for section in image
        .sections
        .iter()
        .filter(|s| !s.executable && !s.writable)
    {
        for offset in (0..section.file_size.saturating_sub(7)).step_by(8) {
            let slot = section.address + offset as u64;
            if let Some(pointer) = image.u64(slot)
                && callers.contains_key(&pointer)
            {
                slots.entry(pointer).or_default().push(slot);
            }
        }
    }
    let mut ordered = Vec::new();
    for (start, calls) in callers {
        let candidates = slots
            .get(&start)
            .context("registration constructor has no initializer pointer")?;
        ensure!(
            candidates.len() == 1,
            "ambiguous initializer order for constructor {start:#x}"
        );
        ordered.push((candidates[0], calls));
    }
    ordered.sort_by_key(|(slot, _)| *slot);
    let first_slot = ordered.first().context("no startup registrations found")?.0;
    let mut table_start = first_slot;
    let mut table_end = first_slot;
    while table_start >= 8
        && image
            .u64(table_start - 8)
            .is_some_and(|pointer| image.executable(pointer))
    {
        table_start -= 8;
    }
    while image
        .u64(table_end + 8)
        .is_some_and(|pointer| image.executable(pointer))
    {
        table_end += 8;
    }
    ensure!(
        ordered
            .iter()
            .all(|(slot, _)| (table_start..=table_end).contains(slot)),
        "registration pointers do not belong to one contiguous startup table"
    );
    let mut interfaces = BTreeMap::<u64, Interface>::new();
    let mut registered_results = BTreeMap::new();
    for (slot, calls) in ordered {
        for call in calls
            .iter()
            .filter(|call| call.target == target || Some(call.target) == range_target)
        {
            if Some(call.target) == range_target {
                let receiver = call.args[0].context("unresolved range receiver")?;
                let result_slot = call.argument_loads[1].context("unresolved range source")?;
                let portion = call.args[2]
                    .filter(|n| *n > 0 && *n <= 254)
                    .context("invalid range portion")?;
                let interface = interfaces
                    .get_mut(&receiver)
                    .context("range before interface registration")?;
                let message = interface
                    .messages
                    .last_mut()
                    .context("range before message registration")?;
                ensure!(
                    registered_results.get(&result_slot) == Some(&(receiver, message.registration)),
                    "range does not refer to last registered message"
                );
                let start = u64::from(message.first_id) + 1;
                ensure!(start <= 254, "cannot expand an exhausted message ID range");
                message.last_id = (start + (254 - start) / portion) as u8;
                message.range_registration = Some(call.address);
                continue;
            }
            let [Some(receiver), Some(name), Some(style), Some(param)] = call.args else {
                anyhow::bail!("unresolved registration arguments at {:#x}", call.address);
            };
            let name = image.string(name).context("invalid message name")?;
            let interface_name = image
                .u64(receiver + 24)
                .and_then(|v| image.string(v))
                .context("unsupported InterfaceMinder layout: missing name at +24")?;
            ensure!(
                interface_name.ends_with("Interface"),
                "unrecognized interface name {interface_name}"
            );
            let framing = match style {
                0 if param <= 1_048_576 => Framing::Fixed {
                    bytes: param as u32,
                },
                1 if (1..=4).contains(&param) => Framing::Variable {
                    prefix_bytes: param as u32,
                },
                2 => Framing::Callback,
                _ => anyhow::bail!("unsupported framing {style}/{param} at {:#x}", call.address),
            };
            let interface = interfaces.entry(receiver).or_insert_with(|| Interface {
                name: interface_name,
                address: receiver,
                messages: Vec::new(),
            });
            let next = interface
                .messages
                .last()
                .map_or(0, |m| u16::from(m.last_id) + 1);
            ensure!(next <= 254, "interface ID overflow");
            interface.messages.push(Message {
                name,
                first_id: next as u8,
                last_id: next as u8,
                framing,
                handler: call.handler.context("unresolved message handler")?,
                registration: call.address,
                initializer_slot: slot,
                range_registration: None,
                callback_length: None,
            });
            if let Some(result_slot) = call.return_store {
                registered_results.insert(result_slot, (receiver, call.address));
            }
        }
    }
    Ok((target, interfaces.into_values().collect()))
}
