use super::fixture::{Rom, advance, sample};
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_md_core::{Emulator, LoadConfig, Region};

const PRODUCTS: [&[u8; 14]; 3] = [b"GM T-81033  00", b"GM T-081326 01", b"GM T-081326 00"];
const PORT: u32 = 0x200000;
const RESULTS: u32 = 0xff2000;

struct Guest {
    rom: Rom,
    lane: u8,
    address_cuts: Vec<u32>,
    send_calls: u8,
}

impl Guest {
    fn new(product: &[u8; 14]) -> Self {
        let mut rom = Rom::new();
        rom.bytes.resize(0x200000, 0);
        rom.bytes[0x180..0x18e].copy_from_slice(product);
        Self {
            rom,
            lane: 0,
            address_cuts: Vec::new(),
            send_calls: 0,
        }
    }

    fn line(&mut self, value: u8) {
        match self.lane {
            0 => self.rom.write(0x33fc, PORT, 0x5a00 | u16::from(value)),
            1 => self.rom.write(0x13fc, PORT, u16::from(value)),
            _ => self.rom.write(0x13fc, PORT + 1, u16::from(value)),
        }
    }

    fn sample(&mut self) -> usize {
        let index = self.rom.results;
        self.rom.word(0x3039);
        self.rom.long(PORT);
        self.rom.word(0x13c0);
        self.rom.long(RESULTS + index as u32);
        self.rom.results += 1;
        index
    }

    fn start(&mut self) {
        self.line(1);
        self.line(3);
        self.line(2);
        self.line(0);
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
            if self.send_calls < 2 && bit == 4 {
                self.address_cuts.push(self.rom.pc as u32);
            }
            self.line(data);
        }
        self.line(1);
        if self.send_calls < 2 {
            self.address_cuts.push(self.rom.pc as u32);
        }
        let ack = self.sample();
        self.line(3);
        if self.send_calls < 2 {
            self.address_cuts.push(self.rom.pc as u32);
        }
        self.line(1);
        self.send_calls = self.send_calls.saturating_add(1);
        ack
    }

    fn receive(&mut self, ack: bool) -> usize {
        let first = self.rom.results;
        for _ in 0..8 {
            self.line(1);
            self.sample();
            self.line(3);
            self.line(1);
        }
        let data = u8::from(!ack);
        self.line(data);
        self.line(data | 2);
        self.line(data);
        first
    }

    fn wait(&mut self) {
        self.rom.word(0x3e3c);
        self.rom.word(0x2fff);
        self.rom.word(0x51cf);
        self.rom.word(0xfffe);
    }

    fn address(&mut self, address: u8) {
        self.start();
        self.send(0xa0);
        self.send(address);
    }

    fn random_read(&mut self, address: u8) {
        self.address(address);
        self.start();
        self.send(0xa1);
    }
}

fn received(machine: &Emulator, first: usize) -> u8 {
    (0..8).fold(0, |value, bit| {
        (value << 1) | ((sample(machine, first + bit) >> 1) & 1)
    })
}

pub fn guest_protocol_and_restore() -> u64 {
    let mut guest = Guest::new(PRODUCTS[1]);
    guest.start();
    let device_ack = guest.send(0xa0);
    let mut cuts = vec![guest.rom.pc as u32];
    let word_ack = guest.send(0xfe);
    cuts.push(guest.rom.pc as u32);
    let data_ack = guest.send(0x96);
    cuts.push(guest.rom.pc as u32);
    guest.send(0x3c);
    let before_stop = guest.rom.pc as u32;
    cuts.push(before_stop);
    guest.stop();
    let busy = guest.rom.pc as u32;
    cuts.push(busy);
    guest.start();
    let busy_nack = guest.send(0xa0);
    guest.stop();
    guest.wait();
    guest.random_read(0xfe);
    cuts.push(guest.rom.pc as u32);
    let fe = guest.receive(true);
    let ff = guest.receive(true);
    let zero = guest.receive(false);
    cuts.push(guest.rom.pc as u32);
    let after_nack = guest.receive(false);
    guest.stop();
    guest.start();
    let ready = guest.send(0xa1);
    let current = guest.receive(false);
    guest.stop();
    guest.lane = 1;
    guest.address(0xff);
    for (lane, value) in [(1, 0x11), (2, 0x22), (1, 0x33), (2, 0x44), (1, 0x55)] {
        guest.lane = lane;
        guest.send(value);
    }
    cuts.push(guest.rom.pc as u32);
    guest.stop();
    guest.wait();
    let mut wrong = Vec::new();
    for command in [0x00, 0xb0, 0xa2, 0xa3, 0xae, 0xaf] {
        guest.start();
        wrong.push(guest.send(command));
        guest.send(0x70);
        guest.send(0x88);
        guest.stop();
    }
    guest.start();
    for value in [1, 0, 1] {
        guest.line(value);
        guest.line(value | 2);
        guest.line(value);
    }
    guest.address(0x70);
    guest.send(0x88);
    guest.line(1);
    guest.line(3);
    guest.line(1);
    cuts.push(guest.rom.pc as u32);
    guest.random_read(0xfc);
    let mut page = Vec::new();
    for index in 0..4 {
        page.push(guest.receive(index != 3));
    }
    guest.stop();
    cuts.extend_from_slice(&guest.address_cuts);
    cuts.sort_unstable();
    cuts.dedup();
    let (bytes, _) = guest.rom.finish();
    let initial: Vec<_> = (0..256).map(|n| n as u8 ^ 0xc7).collect();
    let mut expected = initial.clone();
    expected[0xfc..].copy_from_slice(&[0x22, 0x33, 0x44, 0x55]);
    let mut signature = 0xcbf29ce484222325u64;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let config = LoadConfig {
            region,
            concurrent_sound: true,
        };
        let mut machine = Emulator::new(bytes.clone(), config).unwrap();
        machine.load_save_ram(&initial).unwrap();
        let mut continuations = Vec::new();
        let mut rewind = None;
        for &pc in &cuts {
            advance(&mut machine, Some(pc));
            let raw = machine.encode_state().unwrap();
            let front = machine.encode_frontend_state().unwrap();
            if rewind.is_none() {
                rewind = Some(front.clone());
            }
            if pc == before_stop {
                assert_eq!(machine.dump_save_ram().unwrap(), initial);
            }
            if pc == busy {
                let mut committed = initial.clone();
                committed[0xfe] = 0x96;
                committed[0xff] = 0x3c;
                assert_eq!(machine.dump_save_ram().unwrap(), committed);
            }
            let mut cloned = machine.clone();
            advance(&mut cloned, None);
            continuations.push(cloned.encode_state().unwrap());
            for frontend in [false, true] {
                let mut restored = Emulator::new(bytes.clone(), config).unwrap();
                if frontend {
                    restored.decode_frontend_state(&front).unwrap();
                    assert_eq!(restored.encode_frontend_state().unwrap(), front);
                } else {
                    restored.decode_state(&raw).unwrap();
                    assert_eq!(restored.encode_state().unwrap(), raw);
                }
                advance(&mut restored, None);
                continuations.push(restored.encode_state().unwrap());
            }
        }
        advance(&mut machine, None);
        assert_eq!(machine.dump_save_ram().unwrap(), expected);
        for index in [device_ack, word_ack, data_ack, ready] {
            assert_eq!(sample(&machine, index), 0);
        }
        for &index in &wrong {
            assert_eq!(sample(&machine, index), 2);
        }
        assert_eq!(sample(&machine, busy_nack), 2);
        assert_eq!(received(&machine, fe), 0x96);
        assert_eq!(received(&machine, ff), 0x3c);
        assert_eq!(received(&machine, zero), initial[0]);
        assert_eq!(received(&machine, after_nack), 0xff);
        assert_eq!(received(&machine, current), initial[1]);
        for (index, expected) in page.iter().zip([0x22, 0x33, 0x44, 0x55]) {
            assert_eq!(received(&machine, *index), expected);
        }
        let final_state = machine.encode_state().unwrap();
        assert!(continuations.iter().all(|state| state == &final_state));
        machine.decode_frontend_state(&rewind.unwrap()).unwrap();
        advance(&mut machine, None);
        assert_eq!(machine.encode_state().unwrap(), final_state);
        for value in final_state {
            signature = (signature ^ u64::from(value)).wrapping_mul(0x100000001b3);
        }
    }
    signature
}

pub fn profiles_reset_and_rejection() -> u64 {
    for product in PRODUCTS {
        let mut guest = Guest::new(product);
        guest.start();
        let initial_poll = guest.send(0xa0);
        guest.stop();
        guest.wait();
        guest.random_read(0xfd);
        let reset_read = guest.receive(false);
        guest.stop();
        let before_write = guest.rom.pc as u32;
        guest.address(0xfd);
        guest.send(0x49);
        let inflight = guest.rom.pc as u32;
        guest.stop();
        let busy = guest.rom.pc as u32;
        guest.start();
        let busy_nack = guest.send(0xa0);
        guest.stop();
        guest.wait();
        guest.start();
        let ready = guest.send(0xa1);
        guest.receive(false);
        guest.stop();
        let (bytes, _) = guest.rom.finish();
        for region in [Region::NtscUs, Region::Pal] {
            let config = LoadConfig {
                region,
                concurrent_sound: true,
            };
            let mut machine = Emulator::new(bytes.clone(), config).unwrap();
            assert_eq!(machine.dump_save_ram().unwrap(), [0xff; 256]);
            let raw = machine.encode_state().unwrap();
            for size in [0, 127, 128, 255, 257, 512] {
                assert!(machine.load_save_ram(&vec![0; size]).is_err());
                assert_eq!(machine.encode_state().unwrap(), raw);
            }
            for (offset, value) in [(0x180, b'X'), (0x1b2, 0xf8), (0x1bb, 3)] {
                let mut unsupported = bytes.clone();
                unsupported[offset] = value;
                assert!(Emulator::new(unsupported, LoadConfig::default()).is_err());
            }
            let mut short = bytes.clone();
            short.truncate(0x100000);
            assert!(Emulator::new(short, LoadConfig::default()).is_err());
            advance(&mut machine, Some(inflight));
            let state = machine.encode_state().unwrap();
            let serial = crate::state_layout::serial_range(&state, 256);
            machine.reset();
            let reset = machine.encode_state().unwrap();
            let reset_serial = crate::state_layout::serial_range(&reset, 256);
            assert_eq!(&reset[reset_serial][..15], &state[serial.clone()][..15]);
            machine.decode_state(&state).unwrap();
            machine.reset_cpu().unwrap();
            let after_cpu = machine.encode_state().unwrap();
            assert_eq!(
                &after_cpu[crate::state_layout::serial_range(&after_cpu, 256)],
                &state[serial]
            );
            machine.decode_state(&state).unwrap();
            advance(&mut machine, Some(busy));
            let state = machine.encode_state().unwrap();
            let serial = crate::state_layout::serial_range(&state, 256);
            let start = serial.start;
            let old_until = u64::from_le_bytes(state[start + 15..serial.end].try_into().unwrap());
            let old_now = machine.timing_snapshot().now().get();
            machine.reset();
            let reset = machine.encode_state().unwrap();
            let reset_serial = crate::state_layout::serial_range(&reset, 256);
            let remaining = u64::from_le_bytes(
                reset[reset_serial.start + 15..reset_serial.end]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(remaining, old_until.saturating_sub(old_now));
            assert_eq!(&reset[reset_serial][..15], &state[serial][..15]);
            advance(&mut machine, Some(before_write));
            assert_eq!(sample(&machine, initial_poll), 2);
            assert_eq!(received(&machine, reset_read), 0x49);
            assert_eq!(machine.dump_save_ram().unwrap()[0xfd], 0x49);
            machine.decode_state(&state).unwrap();
            advance(&mut machine, None);
            assert_eq!(sample(&machine, busy_nack), 2);
            assert_eq!(sample(&machine, ready), 0);
            assert_eq!(machine.dump_save_ram().unwrap()[0xfd], 0x49);
            let before = machine.encode_state().unwrap();
            for (offset, value) in [(3, 1), (3, 12), (4, 9), (14, 16), (22, 0xff)] {
                let mut corrupt = state.clone();
                corrupt[start + offset] = value;
                assert!(machine.decode_state(&corrupt).is_err());
                assert_eq!(machine.encode_state().unwrap(), before);
            }
        }
        let mut control = Guest::new(product);
        control.rom.write(0x13fc, 0xa130f1, 1);
        let (bytes, _) = control.rom.finish();
        let mut rejected = Emulator::new(bytes, LoadConfig::default()).unwrap();
        assert!(rejected.step_instruction().is_err());
    }
    let mut sega = Rom::new();
    sega.write(0x13fc, 0xa130f1, 0xff);
    let (bytes, _) = sega.finish();
    let mut machine = Emulator::new(bytes, LoadConfig::default()).unwrap();
    advance(&mut machine, None);
    assert_eq!(machine.dump_save_ram().unwrap(), [0xff; 128]);
    256
}
