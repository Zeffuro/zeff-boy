use std::collections::{BTreeMap, VecDeque};

use super::Budget;
use crate::ScanStop;

const ROM_BYTES: usize = 0x8000;

pub(super) fn reachable_instructions(
    bytes: &[u8],
    budget: &mut Budget<'_, '_>,
) -> Result<BTreeMap<usize, u8>, ScanStop> {
    let bytes = &bytes[..bytes.len().min(ROM_BYTES)];
    let mut pending = VecDeque::from([
        (0x100, 1),
        (0x40, 2),
        (0x48, 4),
        (0x50, 8),
        (0x58, 16),
        (0x60, 32),
    ]);
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
            Flow::Jump(target) => push_if_in_rom(&mut pending, bytes, target, root),
            Flow::Branch(target) | Flow::Call(target) => {
                push_if_in_rom(&mut pending, bytes, target, root);
                push_if_in_rom(&mut pending, bytes, next, root);
            }
            Flow::Next => push_if_in_rom(&mut pending, bytes, next, root),
        }
    }

    Ok(reached)
}

fn push_if_in_rom(pending: &mut VecDeque<(usize, u8)>, bytes: &[u8], at: usize, root: u8) {
    if at < bytes.len() {
        pending.push_back((at, root));
    }
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
    let word = || {
        Some(usize::from(u16::from_le_bytes([
            *bytes.get(at.checked_add(1)?)?,
            *bytes.get(at.checked_add(2)?)?,
        ])))
    };
    let relative = || {
        at.checked_add(2)?
            .checked_add_signed(*bytes.get(at.checked_add(1)?)? as i8 as isize)
    };

    let decoded = match opcode {
        0xc3 => (3, Flow::Jump(word()?)),
        0xc2 | 0xca | 0xd2 | 0xda => (3, Flow::Branch(word()?)),
        0xcd | 0xc4 | 0xcc | 0xd4 | 0xdc => (3, Flow::Call(word()?)),
        0x18 => (2, Flow::Jump(relative()?)),
        0x20 | 0x28 | 0x30 | 0x38 => (2, Flow::Branch(relative()?)),
        0xc7 | 0xcf | 0xd7 | 0xdf | 0xe7 | 0xef | 0xf7 | 0xff => {
            (1, Flow::Call(usize::from(opcode & 0x38)))
        }
        0xc9 | 0xd9 | 0xe9 => (1, Flow::Stop),
        0xd3 | 0xdb | 0xdd | 0xe3 | 0xe4 | 0xeb | 0xec | 0xed | 0xf4 | 0xfc | 0xfd => {
            return None;
        }
        // HALT and STOP can resume after an interrupt; retaining their sequential edge is safe.
        0x10 => (2, Flow::Next),
        0x76 => (1, Flow::Next),
        0x01 | 0x08 | 0x11 | 0x21 | 0x31 | 0xea | 0xfa => (3, Flow::Next),
        0x06 | 0x0e | 0x16 | 0x1e | 0x26 | 0x2e | 0x36 | 0x3e | 0xc6 | 0xce | 0xd6 | 0xde
        | 0xe0 | 0xe6 | 0xe8 | 0xee | 0xf0 | 0xf6 | 0xf8 | 0xfe | 0xcb => (2, Flow::Next),
        _ => (1, Flow::Next),
    };
    (at.checked_add(decoded.0)? <= bytes.len()).then_some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScanLimits;
    use std::collections::BTreeMap;
    use std::sync::atomic::AtomicBool;

    fn reach(bytes: &[u8]) -> BTreeMap<usize, u8> {
        let cancel = AtomicBool::new(false);
        let mut inner = crate::Budget {
            cancel: &cancel,
            remaining: ScanLimits::default().max_work,
        };
        let mut budget =
            Budget::new(&mut inner, ScanLimits::default().max_candidates as usize).unwrap();
        reachable_instructions(bytes, &mut budget).unwrap()
    }

    #[test]
    fn decoder_covers_documented_opcodes_and_lengths() {
        for opcode in 0..=u8::MAX {
            let documented = !matches!(
                opcode,
                0xd3 | 0xdb | 0xdd | 0xe3 | 0xe4 | 0xeb | 0xec | 0xed | 0xf4 | 0xfc | 0xfd
            );
            assert_eq!(
                instruction(&[opcode, 0, 0], 0).is_some(),
                documented,
                "{opcode:02x}"
            );
        }
        for (bytes, expected) in [
            (&[0x10, 0][..], (2, Flow::Next)),
            (&[0xcb, 0x7c][..], (2, Flow::Next)),
            (&[0x08, 0x34, 0x12][..], (3, Flow::Next)),
            (&[0x18, 0xfe][..], (2, Flow::Jump(0))),
            (&[0xc0][..], (1, Flow::Next)),
            (&[0xe9][..], (1, Flow::Stop)),
        ] {
            assert_eq!(instruction(bytes, 0), Some(expected));
            if expected.0 > 1 {
                assert_eq!(instruction(&bytes[..bytes.len() - 1], 0), None);
            }
        }
    }

    #[test]
    fn roots_and_calls_propagate_without_decoding_literals_or_indirect_fallthrough() {
        let mut bytes = vec![0; ROM_BYTES];
        bytes[0x100..0x109]
            .copy_from_slice(&[0x21, 0x34, 0x12, 0xcd, 0x20, 0x01, 0xc3, 0x30, 0x01]);
        bytes[0x40..0x43].copy_from_slice(&[0xc3, 0x20, 0x01]);
        bytes[0x120..0x123].copy_from_slice(&[0xcb, 0x7c, 0xc9]);
        bytes[0x130..0x134].copy_from_slice(&[0xe9, 0xcd, 0x50, 0x01]);
        for at in [0x48, 0x50, 0x58, 0x60] {
            bytes[at] = 0xc9;
        }

        let reached = reach(&bytes);
        assert_eq!(reached[&0x120], 3);
        assert_eq!(reached[&0x40], 2);
        assert_eq!(reached[&0x48], 4);
        assert_eq!(reached[&0x50], 8);
        assert_eq!(reached[&0x58], 16);
        assert_eq!(reached[&0x60], 32);
        assert!(reached.contains_key(&0x106));
        assert!(!reached.contains_key(&0x101));
        assert!(!reached.contains_key(&0x105));
        assert!(!reached.contains_key(&0x131));
        assert!(!reached.contains_key(&0x150));
    }

    #[test]
    fn invalid_and_truncated_instructions_do_not_resynchronize_data() {
        let mut bytes = vec![0; 0x103];
        bytes[0x100..].copy_from_slice(&[0xd3, 0xcd, 0x10]);
        let reached = reach(&bytes);
        assert!(!reached.contains_key(&0x100));
        assert!(!reached.contains_key(&0x101));

        let mut bytes = vec![0; 0x102];
        bytes[0x100..].copy_from_slice(&[0xcd, 0x20]);
        let reached = reach(&bytes);
        assert!(!reached.contains_key(&0x100));
        assert!(!reached.contains_key(&0x101));
    }
}
