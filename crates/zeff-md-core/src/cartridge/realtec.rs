use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::save_state::{StateReader, StateWriter};

const ROM_BYTES: usize = 0x80000;
const IDENTITIES: [[u8; 32]; 3] = [
    [
        0x25, 0x52, 0xa6, 0xfd, 0x12, 0x77, 0x2f, 0x65, 0x0d, 0xd9, 0x1b, 0x8a, 0x66, 0x86, 0xd1,
        0xd4, 0x23, 0xcc, 0xde, 0x64, 0x0b, 0xb7, 0x28, 0x61, 0x1f, 0x3b, 0x6d, 0x8c, 0x74, 0x69,
        0x6e, 0x98,
    ],
    [
        0x8f, 0x36, 0xa8, 0xea, 0x96, 0xbb, 0xb0, 0x9f, 0x8d, 0x4a, 0x2d, 0x7e, 0xfb, 0x77, 0xa3,
        0xc3, 0xda, 0x9a, 0x69, 0xe1, 0x4c, 0x8a, 0x71, 0x4b, 0x40, 0xbc, 0xdb, 0x85, 0x6d, 0xa2,
        0xef, 0x97,
    ],
    [
        0x0e, 0x13, 0x72, 0x67, 0x12, 0x1d, 0x63, 0x40, 0x85, 0x62, 0xda, 0xc2, 0x26, 0xf6, 0x92,
        0xb7, 0x0a, 0x53, 0x99, 0xe2, 0xe6, 0xd3, 0x59, 0xb3, 0xe0, 0xe5, 0x70, 0xca, 0x01, 0x1e,
        0x16, 0xa0,
    ],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mapping {
    Boot,
    Committed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Realtec {
    mapping: Mapping,
    mask: u8,
    value: u8,
}

impl Realtec {
    pub fn detect(rom: &[u8]) -> bool {
        rom.len() == ROM_BYTES && Self::matches_identity(rom.len(), Sha256::digest(rom).into())
    }

    pub fn matches_identity(size: usize, digest: [u8; 32]) -> bool {
        size == ROM_BYTES && IDENTITIES.contains(&digest)
    }

    pub fn new() -> Self {
        Self {
            mapping: Mapping::Boot,
            mask: 0,
            value: 0,
        }
    }

    pub fn source_offset(&self, address: u32) -> Option<usize> {
        if address > 0x3fffff {
            return None;
        }
        let offset = match self.mapping {
            Mapping::Boot => 0x7e000 | (address & 0x1fff),
            Mapping::Committed => {
                let forced = u32::from(self.mask) << 17;
                ((address & 0x7ffff) & !forced) | ((u32::from(self.value) << 17) & forced)
            }
        };
        Some(offset as usize)
    }

    pub(crate) fn cold_prefetch_valid(pc: u32) -> bool {
        let address = pc & zeff_m68k::bus::BUS_ADDRESS_MASK;
        pc & 1 == 0
            && (0..4).all(|offset| {
                let byte = address + offset;
                !(0x100..=0x101).contains(&byte) && Self::new().source_offset(byte).is_some()
            })
    }

    pub fn write_control(&mut self, address: u32, data: u8) {
        // There is no readback; postcommit configuration cannot affect the frozen mapping.
        if self.mapping == Mapping::Committed {
            return;
        }
        match address {
            0x402000 => self.mask = data & 3,
            0x404000 => self.value = data & 3,
            0x400000 if data & 1 != 0 => self.mapping = Mapping::Committed,
            _ => {}
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.mask < 4 && self.value < 4,
            "invalid MD Realtec force bits"
        );
        Ok(())
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.mapping == Mapping::Committed);
        writer.write_u8(self.mask);
        writer.write_u8(self.value);
    }

    pub fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        let next = Self {
            mapping: if reader.read_bool()? {
                Mapping::Committed
            } else {
                Mapping::Boot
            },
            mask: reader.read_u8()?,
            value: reader.read_u8()?,
        };
        next.validate()?;
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Region;
    use crate::cartridge::Cartridge;
    use crate::hardware::bus::Bus;
    use crate::hardware::bus::scheduled::ScheduledBus;
    use crate::hardware::sound::{CandidateBus, SoundFault, SoundMemory, schedule::SoundSchedule};
    use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
    use zeff_m68k::timing::{ClockedBus, CpuClocks};
    use zeff_z80::Z80Bus;

    #[test]
    fn boot_force_bits_commit_lanes_and_reset_share_one_mapping() {
        let mut board = Realtec::new();
        for address in [0, 0x1fff, 0x2000, 0x3fffff] {
            assert_eq!(
                board.source_offset(address),
                Some(0x7e000 | (address as usize & 0x1fff))
            );
        }
        assert_eq!(board.source_offset(0x400000), None);
        for committed in [false, true] {
            for mask in 0..4 {
                for value in 0..4 {
                    let mut original = Realtec::new();
                    original.write_control(0x402000, mask);
                    original.write_control(0x404000, value);
                    if committed {
                        original.write_control(0x400000, 1);
                    }
                    let mut writer = StateWriter::new();
                    original.write_state(&mut writer);
                    let bytes = writer.into_bytes();
                    assert_eq!(bytes, [u8::from(committed), mask, value]);
                    let mut restored = Realtec::new();
                    let mut reader = StateReader::new(&bytes);
                    restored.read_state(&mut reader).unwrap();
                    assert!(reader.is_exhausted());
                    assert_eq!(restored, original);
                    restored.write_control(0x400000, 1);
                    original.write_control(0x400000, 1);
                    assert_eq!(restored, original);
                }
            }
        }
        for bad in [
            vec![],
            vec![1],
            vec![1, 2],
            vec![2, 0, 0],
            vec![0, 4, 0],
            vec![1, 0, 4],
        ] {
            let before = board;
            assert!(board.read_state(&mut StateReader::new(&bad)).is_err());
            assert_eq!(board, before);
        }
        for bad in [Realtec { mask: 4, ..board }, Realtec { value: 4, ..board }] {
            assert!(bad.validate().is_err());
        }
        for mask in 0..4 {
            for value in 0..4 {
                board.reset();
                board.write_control(0x402000, mask | 0xfc);
                board.write_control(0x404000, value | 0xfc);
                board.write_control(0x400000, 0xfe);
                assert_eq!(board.source_offset(0), Some(0x7e000));
                board.write_control(0x400000, 1);
                for bits in 0..4 {
                    let address = (bits << 17) | 0x10123;
                    let mut expected = address;
                    for bit in 0..2 {
                        if mask & (1 << bit) != 0 {
                            let selected = 1 << (bit + 17);
                            expected = (expected & !selected)
                                | (u32::from(value & (1 << bit) != 0) << (bit + 17));
                        }
                    }
                    assert_eq!(board.source_offset(address), Some(expected as usize));
                    assert_eq!(
                        board.source_offset(address | 0x380000),
                        Some(expected as usize)
                    );
                }
                let committed = board;
                for address in [0x400000, 0x402000, 0x404000, 0x402001, 0x402002] {
                    board.write_control(address, 0xff);
                }
                assert_eq!(board, committed);
            }
        }
        let mut rom = vec![0; ROM_BYTES];
        rom[..8].copy_from_slice(&[0, 0xff, 0xf0, 0, 0, 0, 2, 0]);
        rom[0x60000..0x60002].copy_from_slice(&[0x5a, 0xa5]);
        assert!(!Realtec::detect(&rom));
        let mut cart = Cartridge::new(rom).unwrap();
        cart.realtec = Some(Realtec::new());
        let mut bus = Bus::new(cart, Region::NtscUs);
        for (address, lanes, value) in [
            (0x402000, ByteLanes::Both, 0xff02),
            (0x404000, ByteLanes::Upper, 0x0200),
            (0x402001, ByteLanes::Lower, 0x0003),
            (0x402002, ByteLanes::Both, 0x0003),
            (0x400000, ByteLanes::Both, 0x0001),
        ] {
            let access = BusAccess {
                logical_address: address,
                bus_address: address & !1,
                function_code: FunctionCode::SupervisorData,
                kind: AccessKind::Data,
                lanes,
            };
            assert!(bus.write_cycle(access, value, bus.cpu_clock).result.is_ok());
        }
        let board = bus.cartridge.realtec.unwrap();
        assert_eq!(board.source_offset(0x60000), Some(0x60000));
        assert_eq!(bus.cartridge.rom_source_offset(0x60000), Some(0x60000));
        assert_eq!(
            bus.cartridge.read(0x60000),
            bus.cartridge.bytes.get(0x60000).copied()
        );
        assert!(bus.cartridge.mode_sensitive_read(0x60000));
        crate::hardware::vdp::VdpDmaSource::assert_realtec_reads(&bus.cartridge);
        let before = bus.cartridge.realtec;
        let _ = bus.peek(0x100);
        assert_eq!(bus.cartridge.realtec, before);
        for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
            let access = BusAccess {
                logical_address: 0x100 + u32::from(lanes == ByteLanes::Lower),
                bus_address: 0x100,
                function_code: FunctionCode::SupervisorData,
                kind: AccessKind::Data,
                lanes,
            };
            let unchanged = bus.clone();
            let cycle = bus.read_cycle(access, CpuClocks(bus.cpu_clock.0 + 1));
            assert!(matches!(
                cycle.result,
                Err(crate::BusError::ClockMismatch { .. })
            ));
            assert_eq!(bus, unchanged);
            let cycle = bus.read_cycle(access, bus.cpu_clock);
            assert_eq!(cycle.wait_clocks, 0);
            assert!(matches!(
                cycle.result,
                Err(crate::BusError::UnqualifiedCartridgeReset { .. })
            ));
            assert_eq!(bus, unchanged);
            let mut cpu = zeff_z80::Cpu::new();
            let mut schedule = SoundSchedule::default();
            let clock = bus.cpu_clock;
            let cycle =
                ScheduledBus::new(&mut bus, &mut cpu, &mut schedule).read_cycle(access, clock);
            assert_eq!(cycle.wait_clocks, 0);
            assert!(matches!(
                cycle.result,
                Err(crate::BusError::UnqualifiedCartridgeReset { .. })
            ));
            assert_eq!(bus, unchanged);
            assert_eq!(cpu, zeff_z80::Cpu::new());
            assert_eq!(schedule, SoundSchedule::default());
        }
        let mut memory = SoundMemory::default();
        let original = memory.clone();
        {
            let candidate = CandidateBus::new(&mut memory, &bus.cartridge);
            assert_eq!(candidate.cpu_read(0x8100), 0xff);
            assert_eq!(
                candidate.fault.get(),
                Some(SoundFault::UnsupportedAccess {
                    address: 0x8100,
                    direction: zeff_m68k::bus::Direction::Read,
                })
            );
        }
        assert_eq!(memory, original);
        assert_eq!(bus.cartridge.realtec, before);
        bus.cartridge.realtec.as_mut().unwrap().reset();
        assert_eq!(bus.cartridge.rom_source_offset(0x60000), Some(0x7e000));
    }
}
