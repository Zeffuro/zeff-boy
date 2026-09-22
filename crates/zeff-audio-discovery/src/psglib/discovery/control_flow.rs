use std::collections::{BTreeMap, VecDeque};

use super::Budget;
use crate::ScanStop;

pub(super) fn reachable_instructions(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<BTreeMap<usize, u8>, ScanStop> {
    let mut pending = VecDeque::from([(0, 1), (0x38, 2), (0x66, 4)]);
    let mut reached = BTreeMap::<usize, u8>::new();
    while let Some((at, root)) = pending.pop_front() {
        budget.charge(1)?;
        if reached.get(&at).is_some_and(|mask| mask & root != 0) {
            continue;
        }
        let Some((len, flow)) = instruction(bytes, at) else {
            continue;
        };
        *reached.entry(at).or_default() |= root;
        let next = at + len;
        match flow {
            Flow::Stop => {}
            Flow::Jump(target) => pending.push_back((target, root)),
            Flow::Branch(target) | Flow::Call(target) => {
                pending.push_back((target, root));
                pending.push_back((next, root));
            }
            Flow::Next => pending.push_back((next, root)),
        }
    }
    Ok(reached)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    Next,
    Stop,
    Jump(usize),
    Branch(usize),
    Call(usize),
}

fn instruction(bytes: &[u8], at: usize) -> Option<(usize, Flow)> {
    let opcode = *bytes.get(at)?;
    let word = || Some(u16::from_le_bytes([*bytes.get(at + 1)?, *bytes.get(at + 2)?]) as usize);
    let relative = || (at + 2).checked_add_signed(*bytes.get(at + 1)? as i8 as isize);
    let decoded = match opcode {
        0xc3 => (3, Flow::Jump(word()?)),
        0xc2 | 0xca | 0xd2 | 0xda | 0xe2 | 0xea | 0xf2 | 0xfa => (3, Flow::Branch(word()?)),
        0xcd | 0xc4 | 0xcc | 0xd4 | 0xdc | 0xe4 | 0xec | 0xf4 | 0xfc => (3, Flow::Call(word()?)),
        0x18 => (2, Flow::Jump(relative()?)),
        0x10 | 0x20 | 0x28 | 0x30 | 0x38 => (2, Flow::Branch(relative()?)),
        0xc7 | 0xcf | 0xd7 | 0xdf | 0xe7 | 0xef | 0xf7 | 0xff => {
            (1, Flow::Call(usize::from(opcode & 0x38)))
        }
        0xc9 | 0xe9 => (1, Flow::Stop),
        0xcb => (2, Flow::Next),
        0xed => extended(*bytes.get(at + 1)?)?,
        0xdd | 0xfd => indexed(*bytes.get(at + 1)?)?,
        _ => (base_len(opcode), Flow::Next),
    };
    (at.checked_add(decoded.0)? <= bytes.len()).then_some(decoded)
}

fn extended(opcode: u8) -> Option<(usize, Flow)> {
    match opcode {
        0x45 | 0x4d | 0x55 | 0x5d | 0x65 | 0x6d | 0x75 | 0x7d => Some((2, Flow::Stop)),
        0x43 | 0x4b | 0x53 | 0x5b | 0x63 | 0x6b | 0x73 | 0x7b => Some((4, Flow::Next)),
        0x40..=0x42
        | 0x44
        | 0x46..=0x4a
        | 0x4c
        | 0x4e..=0x52
        | 0x54
        | 0x56..=0x5a
        | 0x5c
        | 0x5e..=0x62
        | 0x64
        | 0x66..=0x6a
        | 0x6c
        | 0x6e
        | 0x70..=0x72
        | 0x74
        | 0x76
        | 0x78..=0x7a
        | 0x7c
        | 0x7e
        | 0xa0..=0xa3
        | 0xa8..=0xab
        | 0xb0..=0xb3
        | 0xb8..=0xbb => Some((2, Flow::Next)),
        _ => None,
    }
}

fn indexed(opcode: u8) -> Option<(usize, Flow)> {
    match opcode {
        0xe9 => Some((2, Flow::Stop)),
        0x21 | 0x22 | 0x2a | 0x36 | 0xcb => Some((4, Flow::Next)),
        0x26
        | 0x2e
        | 0x34
        | 0x35
        | 0x46
        | 0x4e
        | 0x56
        | 0x5e
        | 0x66
        | 0x6e
        | 0x70..=0x75
        | 0x77
        | 0x7e
        | 0x86
        | 0x8e
        | 0x96
        | 0x9e
        | 0xa6
        | 0xae
        | 0xb6
        | 0xbe => Some((3, Flow::Next)),
        0x09 | 0x19 | 0x23..=0x25 | 0x29 | 0x2b..=0x2d | 0x39 | 0xe1 | 0xe3 | 0xe5 | 0xf9 => {
            Some((2, Flow::Next))
        }
        // Ignored and repeated prefixes stop this bounded analysis instead of resynchronizing.
        _ => None,
    }
}

fn base_len(opcode: u8) -> usize {
    match opcode {
        0x01 | 0x11 | 0x21 | 0x31 | 0x22 | 0x2a | 0x32 | 0x3a => 3,
        0x06 | 0x0e | 0x16 | 0x1e | 0x26 | 0x2e | 0x36 | 0x3e | 0xc6 | 0xce | 0xd6 | 0xde
        | 0xe6 | 0xee | 0xf6 | 0xfe | 0xd3 | 0xdb => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScanLimits;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn instruction_lengths_and_control_flow_preserve_boundaries() {
        for (bytes, expected) in [
            (&[0xcd, 0x34, 0x12][..], (3, Flow::Call(0x1234))),
            (&[0xd9][..], (1, Flow::Next)),
            (&[0x76][..], (1, Flow::Next)),
            (&[0xed, 0x43, 0x21, 0xcd][..], (4, Flow::Next)),
            (&[0xed, 0x4d][..], (2, Flow::Stop)),
            (&[0xdd, 0xe9][..], (2, Flow::Stop)),
            (&[0xfd, 0x36, 0, 0xcd][..], (4, Flow::Next)),
            (&[0xdd, 0xcb, 0, 0x46][..], (4, Flow::Next)),
            (&[0x28, 2][..], (2, Flow::Branch(4))),
            (&[0xf7][..], (1, Flow::Call(0x30))),
        ] {
            assert_eq!(instruction(bytes, 0), Some(expected));
            if expected.0 > 1 {
                assert_eq!(instruction(&bytes[..bytes.len() - 1], 0), None);
            }
        }
        for bytes in [&[0xed, 0][..], &[0xdd, 0xdd], &[0xfd, 0xcd, 0, 0]] {
            assert_eq!(instruction(bytes, 0), None);
        }
    }

    #[test]
    fn vectors_propagate_roots_without_entering_operands_or_after_indirect_jumps() {
        let mut bytes = vec![0xc9; 0x90];
        bytes[..3].copy_from_slice(&[0xc3, 0x70, 0]);
        bytes[0x38..0x3b].copy_from_slice(&[0xcd, 0x70, 0]);
        bytes[0x70..0x79].copy_from_slice(&[0xed, 0x43, 0x21, 0xcd, 0xdd, 0xe9, 0xcd, 0x80, 0]);
        let cancel = AtomicBool::new(false);
        let mut budget = Budget::new(ScanLimits::default(), &cancel).unwrap();
        let reached = reachable_instructions(&bytes, &mut budget).unwrap();
        assert_eq!(reached[&0x70], 3);
        assert_eq!(reached[&0x74], 3);
        assert_eq!(reached[&0x66], 4);
        assert!(!reached.contains_key(&0x72));
        assert!(!reached.contains_key(&0x76));
        assert!(!reached.contains_key(&0x80));
    }
}
