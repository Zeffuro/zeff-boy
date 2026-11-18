use super::fixture::{DECLARATION, PORT, Rom, advance, sample};
use zeff_emu_common::time::Reset;
use zeff_md_core::{Emulator, LoadConfig, LoadError, Region};

impl Rom {
    fn line(&mut self, value: u8) {
        if self.word_port {
            self.write(0x33fc, 0x3ffffe, u16::from(value));
        } else {
            self.write(0x13fc, PORT, u16::from(value));
        }
    }

    fn mark(&mut self) {
        self.cuts.push(self.pc as u32);
    }

    fn start(&mut self) {
        self.line(3);
        self.line(2);
    }

    fn stop(&mut self) {
        self.line(0);
        self.line(2);
        self.line(3);
    }

    fn send(&mut self, value: u8) -> usize {
        for bit in (0..8).rev() {
            let data = (value >> bit) & 1;
            self.line(data);
            self.line(data | 2);
            if bit == 4 {
                self.mark();
            }
            self.line(data);
        }
        self.line(1);
        self.line(3);
        let ack = self.sample();
        self.mark();
        self.line(1);
        ack
    }

    fn receive(&mut self, ack: bool) -> usize {
        let first = self.results;
        for bit in 0..8 {
            self.line(1);
            self.line(3);
            self.sample();
            if bit == 3 {
                self.mark();
            }
            self.line(1);
        }
        let data = u8::from(!ack);
        self.line(data);
        self.line(data | 2);
        self.mark();
        self.line(data);
        first
    }

    fn wait_write(&mut self) {
        self.word(0x3e3c);
        self.word(0x2fff);
        self.word(0x51cf);
        self.word(0xfffe);
    }
}

fn received(machine: &Emulator, first: usize) -> u8 {
    (0..8).fold(0, |value, bit| (value << 1) | sample(machine, first + bit))
}

pub fn guest_protocol_and_restore() -> u64 {
    protocol_and_restore(Rom::new())
}

fn protocol_and_restore(mut rom: Rom) -> u64 {
    rom.start();
    let address_ack = rom.send(3 << 1);
    let data_ack = rom.send(0xa5);
    rom.send(0x5a);
    rom.send(0xc3);
    rom.mark();
    let before_stop = rom.pc as u32;
    rom.stop();
    rom.mark();
    let after_stop = rom.pc as u32;
    rom.start();
    let busy_nack = rom.send(0);
    rom.stop();
    rom.wait_write();
    rom.start();
    let ready_ack = rom.send((127 << 1) | 1);
    let last = rom.receive(true);
    let wrapped = rom.receive(true);
    let next = rom.receive(false);
    rom.stop();
    rom.start();
    rom.send(8 << 1);
    rom.send(0x77);
    rom.start();
    rom.send((3 << 1) | 1);
    let repeated = rom.receive(false);
    rom.stop();
    rom.mark();
    let before_upper = rom.pc as u32;
    rom.write(0x13fc, 0x200000, 0);
    rom.mark();
    let after_upper = rom.pc as u32;
    rom.word_port = true;
    rom.start();
    let word_ack = rom.send(12 << 1);
    rom.send(0x66);
    rom.stop();
    rom.wait_write();
    rom.start();
    rom.send((12 << 1) | 1);
    let word_value = rom.receive(false);
    rom.stop();
    let (bytes, cuts) = rom.finish();
    let mut signature = 0xcbf29ce484222325u64;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let config = LoadConfig {
            region,
            concurrent_sound: true,
        };
        let mut original = Emulator::new(bytes.clone(), config).unwrap();
        let mut initial: Vec<u8> = (0..128).map(|i| i as u8 ^ 0x96).collect();
        original.load_save_ram(&initial).unwrap();
        let mut restored = Vec::new();
        let mut early = None;
        let mut upper_state = None;
        for &pc in &cuts {
            advance(&mut original, Some(pc));
            let state = original.encode_state().unwrap();
            if early.is_none() {
                early = Some(state.clone());
            }
            if pc == before_stop {
                assert_eq!(original.dump_save_ram().unwrap(), initial);
            }
            if pc == after_stop {
                let mut expected = initial.clone();
                expected[3] = 0xa5;
                expected[0] = 0x5a;
                expected[1] = 0xc3;
                assert_eq!(original.dump_save_ram().unwrap(), expected);
            }
            if pc == before_upper {
                upper_state = Some(state[crate::state_layout::serial_range(&state, 128)].to_vec());
            }
            if pc == after_upper {
                assert_eq!(
                    upper_state.as_deref(),
                    Some(&state[crate::state_layout::serial_range(&state, 128)])
                );
            }
            let mut copy = Emulator::new(bytes.clone(), config).unwrap();
            copy.decode_state(&state).unwrap();
            assert_eq!(copy.encode_state().unwrap(), state);
            advance(&mut copy, None);
            restored.push(copy.encode_state().unwrap());
        }
        advance(&mut original, None);
        initial[3] = 0xa5;
        initial[0] = 0x5a;
        initial[1] = 0xc3;
        initial[12] = 0x66;
        assert_eq!(original.dump_save_ram().unwrap(), initial);
        assert_eq!(sample(&original, address_ack), 0);
        assert_eq!(sample(&original, data_ack), 0);
        assert_eq!(sample(&original, busy_nack), 1);
        assert_eq!(sample(&original, ready_ack), 0);
        assert_eq!(received(&original, last), 127 ^ 0x96);
        assert_eq!(received(&original, wrapped), 0x5a);
        assert_eq!(received(&original, next), 0xc3);
        assert_eq!(received(&original, repeated), 0xa5);
        assert_eq!(sample(&original, word_ack), 0);
        assert_eq!(received(&original, word_value), 0x66);
        let final_state = original.encode_state().unwrap();
        assert!(restored.iter().all(|state| state == &final_state));
        original.decode_state(&early.unwrap()).unwrap();
        advance(&mut original, None);
        assert_eq!(original.encode_state().unwrap(), final_state);
        for value in final_state {
            signature = (signature ^ u64::from(value)).wrapping_mul(0x100000001b3);
        }
    }
    signature
}

pub fn admission_persistence_and_rejection() -> u64 {
    let (bytes, _) = Rom::new().finish();
    let mut machine = Emulator::new(bytes.clone(), LoadConfig::default()).unwrap();
    assert_eq!(machine.dump_save_ram().unwrap(), [0xff; 128]);
    let state = machine.encode_state().unwrap();
    for size in [0, 127, 129, 512] {
        assert!(machine.load_save_ram(&vec![0; size]).is_err());
        assert_eq!(machine.encode_state().unwrap(), state);
    }
    let cells: Vec<_> = (0..128).map(|n| n as u8).collect();
    machine.load_save_ram(&cells).unwrap();
    machine.reset();
    assert_eq!(machine.dump_save_ram().unwrap(), cells);
    for offset in [0, 0x200000, 0x3ffffe] {
        if offset != 0 {
            assert_eq!(machine.peek(offset + 1), Some(1));
        }
    }
    for (offset, value) in [(0x180, b'X'), (0x1b2, 0xf8), (0x1bb, 3)] {
        let mut unsupported = bytes.clone();
        unsupported[offset] = value;
        assert!(Emulator::new(unsupported, LoadConfig::default()).is_err());
    }
    let before = machine.encode_state().unwrap();
    for changes in [
        vec![(3, 8)],
        vec![(4, 9)],
        vec![(14, 16)],
        vec![(3, 2), (4, 9), (0, 0), (2, 1), (8, 1)],
        vec![(3, 5), (0, 1), (7, 1)],
        vec![(22, 0xff)],
    ] {
        let mut corrupted = state.clone();
        let start = crate::state_layout::serial_range(&corrupted, 128).start;
        for (offset, value) in changes {
            corrupted[start + offset] = value;
        }
        assert!(machine.decode_state(&corrupted).is_err());
        assert_eq!(machine.encode_state().unwrap(), before);
    }
    128
}

pub fn header_address_profile_and_restore() -> u64 {
    let mut rom = Rom::new();
    rom.bytes.resize(0x100000, 0);
    rom.bytes[0x180..0x18e].copy_from_slice(b"MK 00001211-00");
    rom.bytes[0x1b0..0x1bc].copy_from_slice(b"RA\xe8\x40\x00\x03\x0d\x41\x00\x03\x0d\x41");
    let mut machine = Emulator::new(rom.bytes.clone(), LoadConfig::default()).unwrap();
    assert_eq!(machine.dump_save_ram().unwrap(), [0xff; 128]);
    let state = machine.encode_state().unwrap();
    for size in [0, 127, 129, 256] {
        assert!(machine.load_save_ram(&vec![0; size]).is_err());
        assert_eq!(machine.encode_state().unwrap(), state);
    }
    let cells: Vec<u8> = (0u8..128).map(|n| n.wrapping_mul(73) ^ 0x19).collect();
    machine.load_save_ram(&cells).unwrap();
    machine.reset();
    assert_eq!(machine.dump_save_ram().unwrap(), cells);
    for size in [0x80000, 0x100001, 0x200000] {
        let mut other = rom.bytes.clone();
        other.resize(size, 0);
        assert!(matches!(
            Emulator::new(other, LoadConfig::default()),
            Err(LoadError::UnsupportedSaveHardware)
        ));
    }
    for (offset, value) in [
        (0x180, b'G'),
        (0x18d, b'1'),
        (0x1b2, 0xf8),
        (0x1b7, 0x40),
        (0x1bb, 0x40),
    ] {
        let mut other = rom.bytes.clone();
        other[offset] = value;
        assert!(matches!(
            Emulator::new(other, LoadConfig::default()),
            Err(LoadError::UnsupportedSaveHardware)
        ));
    }
    let mut other = rom.bytes.clone();
    other[0x1b0..0x1bc].copy_from_slice(DECLARATION);
    assert!(matches!(
        Emulator::new(other, LoadConfig::default()),
        Err(LoadError::UnsupportedSaveHardware)
    ));
    protocol_and_restore(rom)
}
