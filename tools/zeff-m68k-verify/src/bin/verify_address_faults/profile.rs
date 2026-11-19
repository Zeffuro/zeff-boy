use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, Direction};
use zeff_m68k_verify::reference::Case;

#[path = "profile/cmp.rs"]
mod cmp;
#[path = "profile/cmpi.rs"]
mod cmpi;
#[path = "profile/data_arithmetic.rs"]
mod data_arithmetic;
#[path = "profile/immediate.rs"]
mod immediate;
#[path = "profile/memory.rs"]
mod memory;
#[path = "profile/status.rs"]
mod status;
#[path = "profile/tst.rs"]
mod tst;

pub(super) struct Profile {
    pub initial: Registers,
    pub phase: Registers,
    pub operand: u32,
    pub direction: Direction,
    pub marker_word: Option<u16>,
    pub marker: usize,
    pub prefix_clocks: u64,
    pub prefix: Vec<Prefix>,
    pub operand_program: bool,
    pub capture_known: bool,
}

pub(super) enum Prefix {
    Idle(u32),
    Read(u32),
    DataRead(u32, bool),
}

impl Prefix {
    pub(super) fn read_access(&self) -> Option<(u32, AccessKind, bool)> {
        match *self {
            Self::Read(address) => Some((address, AccessKind::Prefetch, true)),
            Self::DataRead(address, program) => Some((address, AccessKind::Data, program)),
            Self::Idle(_) => None,
        }
    }
}

pub(super) fn legal_tst(word: u16) -> bool {
    let mode = word >> 3 & 7;
    word & 0xff00 == 0x4a00
        && word >> 6 & 3 != 3
        && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && word & 7 <= 1)
}

pub(super) fn known(word: u16) -> bool {
    if word == 0x4e71
        || word & 0xf100 == 0x7000
        || word >> 12 == 6
        || legal_status(word)
        || legal_tst(word)
        || legal_cmp(word)
        || legal_cmpi(word)
        || legal_cmpa(word)
        || legal_address_arithmetic(word)
        || legal_data_arithmetic(word)
    {
        return true;
    }
    if !matches!(word >> 12, 1..=3) {
        return false;
    }
    let source = (word >> 3) & 7;
    let destination = (word >> 6) & 7;
    !(source == 7 && word & 7 > 4
        || destination == 7 && (word >> 9) & 7 > 1
        || word >> 12 == 1 && (source == 1 || destination == 1))
}

pub(super) fn legal_status(word: u16) -> bool {
    let mode = word >> 3 & 7;
    matches!(word & 0xffc0, 0x44c0 | 0x46c0)
        && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && word & 7 <= 4)
}

pub(super) fn legal_cmp(word: u16) -> bool {
    let size = word >> 6 & 7;
    let mode = word >> 3 & 7;
    word >> 12 == 11 && size <= 2 && !(size == 0 && mode == 1) && !(mode == 7 && word & 7 > 4)
}

pub(super) fn legal_cmpa(word: u16) -> bool {
    word >> 12 == 11 && matches!(word >> 6 & 7, 3 | 7) && !(word >> 3 & 7 == 7 && word & 7 > 4)
}

pub(super) fn legal_cmpi(word: u16) -> bool {
    let mode = word >> 3 & 7;
    word & 0xff00 == 0x0c00
        && word >> 6 & 3 != 3
        && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && word & 7 <= 1)
}

pub(super) fn legal_address_arithmetic(word: u16) -> bool {
    matches!(word >> 12, 9 | 13)
        && matches!(word >> 6 & 7, 3 | 7)
        && !(word >> 3 & 7 == 7 && word & 7 > 4)
}

pub(super) fn legal_data_arithmetic(word: u16) -> bool {
    data_arithmetic::legal(word)
}

pub(super) fn data_cohort_matches(case: &Case) -> bool {
    data_arithmetic::cohort_matches(case)
}

impl Profile {
    pub(super) fn new(case: &Case) -> Result<Self, String> {
        let initial = case.initial.registers()?;
        let opcode = case.initial.prefetch[0];
        if super::intake::selected_status_memory(case) {
            return status::profile(case, initial);
        }
        if super::intake::selected_cmpi(case) {
            return cmpi::profile(case, initial);
        }
        if super::intake::selected_tst(case) {
            return tst::profile(case, initial);
        }
        if super::intake::selected_cmp(case)
            || super::intake::selected_cmpa(case)
            || super::intake::selected_address_arithmetic(case)
            || super::intake::selected_data_arithmetic(case)
        {
            return cmp::profile(case, initial);
        }
        let source_mode = (opcode >> 3) & 7;
        if source_mode == 7 && opcode & 7 == 4 {
            return immediate::profile(case, initial);
        }
        if source_mode >= 2 && (opcode >> 6) & 7 >= 2 {
            return memory::profile(case, initial);
        }
        let write = source_mode <= 1;
        let mode = if write {
            (opcode >> 6) & 7
        } else {
            (opcode >> 3) & 7
        };
        let an = usize::from(if write { (opcode >> 9) & 7 } else { opcode & 7 });
        let base = initial.address_register(an).unwrap();
        let long = opcode >> 12 == 2;
        let stride = if long { 4 } else { 2 };
        let value = if source_mode == 1 {
            initial.address_register(usize::from(opcode & 7)).unwrap()
        } else {
            initial.d[usize::from(opcode & 7)]
        };
        if mode >= 5 {
            return Self::extension(case, initial, write, (mode, an), long, value);
        }
        let mut phase = initial;
        if (!write && mode == 3 && !long) || mode == 4 && (!write || !long) {
            phase.set_address_register(
                an,
                if mode == 3 {
                    base.wrapping_add(stride)
                } else {
                    base.wrapping_sub(stride)
                },
            );
        }
        if write && (!long || mode == 4) {
            let operand = if long { value } else { value & 65535 };
            let sign = if long { 0x8000_0000 } else { 32768 };
            let flags = if operand == 0 {
                4
            } else if operand & sign != 0 {
                8
            } else {
                0
            };
            phase.set_status_register((initial.status_register() & !15) | flags);
        }
        let operand = if mode == 4 {
            base.wrapping_sub(if write && long { 2 } else { stride })
        } else {
            base
        };
        let marker_word = write.then_some(if !long || mode == 4 {
            value as u16
        } else {
            (value >> 16) as u16
        });
        let ird = if write && mode == 4 && !long {
            case.initial.prefetch[1]
        } else {
            opcode
        };
        Ok(Self {
            initial,
            phase,
            operand,
            direction: if write {
                Direction::Write
            } else {
                Direction::Read
            },
            marker_word,
            marker: usize::from(mode == 4),
            prefix_clocks: if mode != 4 {
                0
            } else if write {
                4
            } else {
                2
            },
            prefix: if mode != 4 {
                vec![]
            } else if write {
                vec![Prefix::Read(initial.pc.wrapping_add(4))]
            } else {
                vec![Prefix::Idle(2)]
            },
            operand_program: false,
            capture_known: known(ird),
        })
    }

    fn extension(
        case: &Case,
        initial: Registers,
        write: bool,
        address: (u16, usize),
        long: bool,
        value: u32,
    ) -> Result<Self, String> {
        let (mode, register) = address;
        let extension = case.initial.prefetch[1];
        let base = if mode == 7 {
            initial.pc.wrapping_add(2)
        } else {
            initial.address_register(register).unwrap()
        };
        let mut prefix = Vec::new();
        if mode == 6 || mode == 7 && register == 3 {
            prefix.push(Prefix::Idle(2));
        }
        prefix.push(Prefix::Read(initial.pc.wrapping_add(4)));
        let operand = match (mode, register) {
            (5, _) | (7, 2) => base.wrapping_add_signed(i32::from(extension as i16)),
            (6, _) | (7, 3) => {
                let index = usize::from((extension >> 12) & 7);
                let index = if extension & 0x8000 == 0 {
                    initial.d[index]
                } else {
                    initial.address_register(index).unwrap()
                };
                let index = if extension & 0x0800 == 0 {
                    i32::from(index as i16) as u32
                } else {
                    index
                };
                base.wrapping_add(index)
                    .wrapping_add_signed(i32::from(extension as u8 as i8))
            }
            (7, 0) => i32::from(extension as i16) as u32,
            (7, 1) => {
                let address = initial.pc.wrapping_add(4) & 0x00ff_fffe;
                let byte = |address| {
                    case.initial
                        .ram
                        .iter()
                        .find(|(at, _)| *at == address)
                        .map(|(_, value)| u32::from(*value))
                        .ok_or("missing absolute-long extension RAM")
                };
                prefix.push(Prefix::Read(initial.pc.wrapping_add(6)));
                (u32::from(extension) << 16) | (byte(address)? << 8) | byte(address + 1)?
            }
            _ => return Err("unsupported extension address-fault route".into()),
        };
        let mut phase = initial;
        if write {
            let sr = initial.status_register();
            let flags = if !long {
                if value & 65535 == 0 {
                    4
                } else if value & 32768 != 0 {
                    8
                } else {
                    0
                }
            } else if mode == 5 || mode == 6 {
                let flags = sr & !12;
                flags
                    | if value & 0x8000_0000 != 0 { 8 } else { 0 }
                    | if value >> 16 == 0 { sr & 4 } else { 0 }
            } else if value == 0 {
                4
            } else if value & 0x8000_0000 != 0 {
                8
            } else {
                0
            };
            phase.set_status_register(if long && mode != 7 {
                flags
            } else {
                (sr & !15) | flags
            });
        }
        let prefix_clocks = prefix
            .iter()
            .map(|p| {
                u64::from(match p {
                    Prefix::Idle(clocks) => *clocks,
                    Prefix::Read(_) | Prefix::DataRead(_, _) => 4,
                })
            })
            .sum();
        Ok(Self {
            initial,
            phase,
            operand,
            direction: if write {
                Direction::Write
            } else {
                Direction::Read
            },
            marker_word: write.then_some(if long {
                (value >> 16) as u16
            } else {
                value as u16
            }),
            marker: prefix.len(),
            prefix_clocks,
            prefix,
            operand_program: mode == 7 && matches!(register, 2 | 3),
            capture_known: true,
        })
    }
}
