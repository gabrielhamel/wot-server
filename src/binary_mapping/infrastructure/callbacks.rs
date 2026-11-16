use super::pe::Image;
use crate::binary_mapping::domain::{CallbackLength, Framing, Interface};
use iced_x86::{Decoder, DecoderOptions, Instruction, Mnemonic, OpKind, Register};
use std::collections::BTreeMap;

fn register_value(registers: &BTreeMap<Register, u64>, register: Register) -> Option<u64> {
    let value = *registers.get(&register.full_register())?;
    Some(match register.size() {
        1 => value & 0xff,
        2 => value & 0xffff,
        4 => value & 0xffff_ffff,
        8 => value,
        _ => return None,
    })
}

fn address(instruction: &Instruction, registers: &BTreeMap<Register, u64>) -> Option<u64> {
    if instruction.is_ip_rel_memory_operand() {
        return None;
    }
    let base = if instruction.memory_base() == Register::None {
        0
    } else {
        register_value(registers, instruction.memory_base())?
    };
    let index = if instruction.memory_index() == Register::None {
        0
    } else {
        register_value(registers, instruction.memory_index())?
    };
    Some(
        base.wrapping_add(index.wrapping_mul(instruction.memory_index_scale() as u64))
            .wrapping_add(instruction.memory_displacement64()),
    )
}

fn operand(
    instruction: &Instruction,
    index: u32,
    registers: &BTreeMap<Register, u64>,
    image: &Image<'_>,
    handler: u64,
) -> Option<u64> {
    match instruction.op_kind(index) {
        OpKind::Register => register_value(registers, instruction.op_register(index)),
        OpKind::Memory => {
            let at = address(instruction, registers)?;
            let size = instruction.memory_size().size();
            if at < handler || at.checked_add(size as u64)? > handler.checked_add(256)? {
                return None;
            }
            let bytes = image.slice(at, size)?;
            let mut value = [0; 8];
            value.get_mut(..size)?.copy_from_slice(bytes);
            Some(u64::from_le_bytes(value))
        }
        OpKind::Immediate8
        | OpKind::Immediate16
        | OpKind::Immediate32
        | OpKind::Immediate64
        | OpKind::Immediate8to16
        | OpKind::Immediate8to32
        | OpKind::Immediate8to64
        | OpKind::Immediate32to64 => Some(instruction.immediate(index)),
        _ => None,
    }
}

fn evaluate(image: &Image<'_>, function: u64, handler: u64) -> Option<u32> {
    let mut registers = BTreeMap::from([(Register::RCX, handler)]);
    let mut zero = None;
    let mut ip = function;
    for _ in 0..128 {
        let bytes = image.slice(ip, 15)?;
        if !image.executable(ip) || ip.abs_diff(function) > 512 {
            return None;
        }
        let instruction = Decoder::with_ip(64, bytes, ip, DecoderOptions::NONE).decode();
        ip = instruction.next_ip();
        match instruction.mnemonic() {
            Mnemonic::Ret => {
                return register_value(&registers, Register::EAX)
                    .and_then(|v| u32::try_from(v).ok())
                    .filter(|v| *v <= 1_048_576);
            }
            Mnemonic::Je | Mnemonic::Jne => {
                if zero? == (instruction.mnemonic() == Mnemonic::Je) {
                    ip = instruction.near_branch_target();
                }
            }
            Mnemonic::Jmp if instruction.op0_kind() == OpKind::NearBranch64 => {
                ip = instruction.near_branch_target()
            }
            Mnemonic::Cmp | Mnemonic::Test => {
                let left = operand(&instruction, 0, &registers, image, handler)?;
                let right = operand(&instruction, 1, &registers, image, handler)?;
                zero = Some(if instruction.mnemonic() == Mnemonic::Cmp {
                    left == right
                } else {
                    left & right == 0
                });
            }
            Mnemonic::Mov
            | Mnemonic::Movzx
            | Mnemonic::Lea
            | Mnemonic::Xor
            | Mnemonic::Add
            | Mnemonic::Sub => {
                if instruction.op0_kind() != OpKind::Register {
                    return None;
                }
                let destination = instruction.op0_register();
                let value = match instruction.mnemonic() {
                    Mnemonic::Lea => address(&instruction, &registers)?,
                    Mnemonic::Mov | Mnemonic::Movzx => {
                        operand(&instruction, 1, &registers, image, handler)?
                    }
                    Mnemonic::Xor if instruction.op0_register() == instruction.op1_register() => 0,
                    operation => {
                        let left = operand(&instruction, 0, &registers, image, handler)?;
                        let right = operand(&instruction, 1, &registers, image, handler)?;
                        match operation {
                            Mnemonic::Xor => left ^ right,
                            Mnemonic::Add => left.wrapping_add(right),
                            Mnemonic::Sub => left.wrapping_sub(right),
                            _ => return None,
                        }
                    }
                };
                if destination.size() < 4 {
                    return None;
                }
                let value = if destination.size() == 4 {
                    value & 0xffff_ffff
                } else {
                    value
                };
                registers.insert(destination.full_register(), value);
                if matches!(
                    instruction.mnemonic(),
                    Mnemonic::Xor | Mnemonic::Add | Mnemonic::Sub
                ) {
                    zero = Some(value == 0);
                }
            }
            Mnemonic::Nop => (),
            _ => return None,
        }
    }
    None
}

pub fn resolve(image: &Image<'_>, interfaces: &mut [Interface]) {
    for message in interfaces.iter_mut().flat_map(|i| &mut i.messages) {
        if !matches!(message.framing, Framing::Callback) || message.handler == 0 {
            continue;
        }
        let callback = image
            .u64(message.handler)
            .and_then(|table| image.u64(table + 3 * 8));
        if let Some(function) = callback {
            message.callback_length = evaluate(image, function, message.handler)
                .map(|bytes| CallbackLength { bytes, function });
        }
    }
}
