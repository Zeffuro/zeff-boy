use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::physical_instruction::InstructionPhase;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Read(BusAccess, u64, u16, InstructionPhase),
    Idle(u64, u32),
}

pub struct Plan {
    pub registers: Registers,
    pub queue: [u16; 2],
    pub clock: u64,
    pub events: Vec<Event>,
}

pub fn admitted(word: u16) -> bool {
    let (op, mode, reg) = (word >> 6 & 7, word >> 3 & 7, word & 7);
    word >> 12 == 11
        && (op <= 2 || matches!(op, 3 | 7))
        && (mode != 7 || reg <= 4)
        && !(op == 0 && mode == 1)
        && (op == 0 || mode <= 1 || mode == 7 && reg == 4)
}

fn flags(sr: u16, source: u32, destination: u32, bits: u32) -> u16 {
    let modulus = 1u64 << bits;
    let mask = (modulus - 1) as u32;
    let sign = 1u32 << (bits - 1);
    let (s, d) = (source & mask, destination & mask);
    let result = (u64::from(d) + modulus - u64::from(s)) as u32 & mask;
    (sr & !15)
        | u16::from(result == 0) << 2
        | u16::from(result & sign != 0) << 3
        | u16::from((d ^ s) & (d ^ result) & sign != 0) << 1
        | u16::from(s > d)
}

struct Reads<'a, F: Fn(u32) -> u16> {
    sample: &'a F,
    bank: bool,
    clock: u64,
    elapsed: u32,
    events: Vec<Event>,
}

impl<F: Fn(u32) -> u16> Reads<'_, F> {
    fn read(
        &mut self,
        address: u32,
        program: bool,
        prefetch: bool,
        byte: bool,
        phase: InstructionPhase,
    ) -> u16 {
        let access = BusAccess {
            logical_address: address,
            bus_address: address & 0xfffffe,
            function_code: match (self.bank, program) {
                (false, false) => FunctionCode::UserData,
                (false, true) => FunctionCode::UserProgram,
                (true, false) => FunctionCode::SupervisorData,
                (true, true) => FunctionCode::SupervisorProgram,
            },
            kind: if prefetch {
                AccessKind::Prefetch
            } else {
                AccessKind::Data
            },
            lanes: if !byte {
                ByteLanes::Both
            } else if address & 1 == 0 {
                ByteLanes::Upper
            } else {
                ByteLanes::Lower
            },
        };
        let value = (self.sample)(access.bus_address);
        self.events
            .push(Event::Read(access, self.clock, value, phase));
        self.clock += u64::from(self.elapsed);
        value
    }
    fn idle(&mut self, clocks: u32) {
        self.events.push(Event::Idle(self.clock, clocks));
        self.clock += u64::from(clocks);
    }
}

pub fn plan(
    word: u16,
    original: Registers,
    retained: u16,
    start: u64,
    elapsed: u32,
    sample: impl Fn(u32) -> u16,
) -> Plan {
    assert!(admitted(word));
    let (op, mode, register, destination) = (
        word >> 6 & 7,
        word >> 3 & 7,
        usize::from(word & 7),
        usize::from(word >> 9 & 7),
    );
    let cmpa = matches!(op, 3 | 7);
    let bits = match op {
        0 => 8,
        1 | 3 => 16,
        2 | 7 => 32,
        _ => unreachable!(),
    };
    let immediate = mode == 7 && register == 4;
    let memory = mode >= 2 && !immediate;
    let extensions = if mode == 7 && (register == 1 || immediate && bits == 32) {
        2
    } else {
        usize::from(mode >= 5)
    };
    let mut reads = Reads {
        sample: &sample,
        bank: original.supervisor(),
        clock: start,
        elapsed,
        events: Vec::new(),
    };
    if memory && (matches!(mode, 4 | 6) || mode == 7 && register == 3) {
        reads.idle(2);
    }
    let mut stream = retained;
    let mut extension_words = Vec::new();
    for index in 0..extensions {
        extension_words.push(stream);
        let phase = match (immediate, index) {
            (true, 0) => InstructionPhase::ImmediateFirst,
            (true, _) => InstructionPhase::ImmediateSecond,
            (false, 0) => InstructionPhase::ExtensionFirst,
            (false, _) => InstructionPhase::ExtensionSecond,
        };
        stream = reads.read(
            original.pc.wrapping_add(4 + index as u32 * 2),
            true,
            true,
            false,
            phase,
        );
    }
    let mut registers = original;
    let source = if mode == 0 {
        original.d[register]
    } else if mode == 1 {
        original.address_register(register).unwrap()
    } else if immediate {
        if bits == 32 {
            (u32::from(extension_words[0]) << 16) | u32::from(extension_words[1])
        } else {
            u32::from(extension_words[0])
        }
    } else {
        assert_eq!(bits, 8);
        let delta = if register == 7 { 2 } else { 1 };
        let base = if mode == 7 && matches!(register, 2 | 3) {
            original.pc.wrapping_add(2)
        } else {
            original.address_register(register).unwrap()
        };
        let address = match (mode, register) {
            (2 | 3, _) => base,
            (4, _) => base.wrapping_sub(delta),
            (5, _) | (7, 2) => base.wrapping_add(extension_words[0] as i16 as i32 as u32),
            (6, _) | (7, 3) => {
                let extension = extension_words[0];
                let index = usize::from(extension >> 12 & 7);
                let value = if extension & 0x8000 == 0 {
                    original.d[index]
                } else {
                    original.address_register(index).unwrap()
                };
                let value = if extension & 0x800 == 0 {
                    value as i16 as i32 as u32
                } else {
                    value
                };
                base.wrapping_add(value)
                    .wrapping_add(extension as i8 as i32 as u32)
            }
            (7, 0) => extension_words[0] as i16 as i32 as u32,
            (7, 1) => (u32::from(extension_words[0]) << 16) | u32::from(extension_words[1]),
            _ => unreachable!(),
        };
        if mode == 3 {
            registers.set_address_register(register, address.wrapping_add(delta));
        } else if mode == 4 {
            registers.set_address_register(register, address);
        }
        let value = reads.read(
            address,
            mode == 7 && matches!(register, 2 | 3),
            false,
            true,
            InstructionPhase::OperandReadHigh,
        );
        u32::from(if address & 1 == 0 {
            value >> 8
        } else {
            value & 255
        })
    };
    let source = if cmpa && bits == 16 {
        source as i16 as i32 as u32
    } else {
        source
    };
    let compared = if cmpa {
        registers.address_register(destination).unwrap()
    } else {
        original.d[destination]
    };
    registers.set_status_register(flags(
        original.status_register(),
        source,
        compared,
        if cmpa { 32 } else { bits },
    ));
    let final_word = reads.read(
        original.pc.wrapping_add(4 + extensions as u32 * 2),
        true,
        true,
        false,
        InstructionPhase::OperandRefill,
    );
    if !memory && (cmpa || bits == 32) {
        reads.idle(2);
    }
    registers.pc = original.pc.wrapping_add(2 + extensions as u32 * 2);
    Plan {
        registers,
        queue: [stream, final_word],
        clock: reads.clock,
        events: reads.events,
    }
}
