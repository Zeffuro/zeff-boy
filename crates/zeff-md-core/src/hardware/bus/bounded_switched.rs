use super::*;
use zeff_m68k::bus::FunctionCode;

impl Bus {
    #[inline]
    pub(super) fn preflight_bounded_switched(
        &self,
        access: BusAccess,
        value: Option<u16>,
    ) -> Result<(), BusError> {
        let logical = access.logical_address & BUS_ADDRESS_MASK;
        if !self.cartridge.bounded_switched_guarded(access.bus_address)
            && ((logical & !1) == access.bus_address
                || !self.cartridge.bounded_switched_guarded(logical))
        {
            return Ok(());
        }
        self.qualify_bounded_switched(access, logical, value)
    }

    #[cold]
    #[inline(never)]
    fn qualify_bounded_switched(
        &self,
        access: BusAccess,
        logical: u32,
        value: Option<u16>,
    ) -> Result<(), BusError> {
        let data = access.kind == AccessKind::Data
            && matches!(
                access.function_code,
                FunctionCode::UserData | FunctionCode::SupervisorData
            );
        let lower =
            access.lanes == ByteLanes::Lower && logical == access.bus_address.wrapping_add(1);
        let canonical = access.bus_address & 1 == 0
            && match access.lanes {
                ByteLanes::Lower => lower,
                ByteLanes::Upper | ByteLanes::Both => logical == access.bus_address,
            };
        let save = self
            .cartridge
            .save
            .as_ref()
            .filter(|save| save.partial_switched());
        let qualified = canonical
            && save.is_some_and(|save| {
                if logical == crate::cartridge::ram::SAVE_CONTROL + 1 {
                    return save.writable_partial_switched()
                        && data
                        && lower
                        && value.is_some_and(|value| value <= 3);
                }
                if !(0x200000..0x400000).contains(&logical) {
                    return false;
                }
                if save.rom_visible() {
                    let program = matches!(
                        access.kind,
                        AccessKind::Opcode | AccessKind::Extension | AccessKind::Prefetch
                    ) && matches!(
                        access.function_code,
                        FunctionCode::UserProgram | FunctionCode::SupervisorProgram
                    );
                    return value.is_none()
                        && (data || program)
                        && (access.bus_address as usize + 1) < self.cartridge.bytes.len();
                }
                data && match access.lanes {
                    ByteLanes::Upper => save.bounded_bytes_byte(logical),
                    ByteLanes::Lower => save.partial_switched_byte(logical),
                    ByteLanes::Both => false,
                }
            });
        if qualified {
            Ok(())
        } else {
            Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction: if value.is_some() {
                    Direction::Write
                } else {
                    Direction::Read
                },
            })
        }
    }

    pub(super) fn read_bounded_switched(
        &self,
        access: BusAccess,
    ) -> Option<Result<InstructionInput, BusError>> {
        self.cartridge
            .bounded_switched_guarded(access.bus_address)
            .then(|| {
                self.preflight_bounded_switched(access, None)?;
                let missing = || BusError::UnsupportedAccess {
                    address: access.bus_address,
                    direction: Direction::Read,
                };
                let save = self.cartridge.save.as_ref().ok_or_else(missing)?;
                if !save.rom_visible() {
                    let address = if save.bounded_bytes() {
                        access.logical_address & BUS_ADDRESS_MASK
                    } else {
                        access.bus_address + 1
                    };
                    let byte = save.read(address).ok_or_else(missing)?;
                    return Ok(InstructionInput::unknown(
                        if access.lanes == ByteLanes::Upper {
                            u16::from(byte) << 8
                        } else {
                            u16::from(byte)
                        },
                    ));
                }
                let upper = self
                    .cartridge
                    .source_sample(access.bus_address as usize)
                    .ok_or_else(missing)?;
                let lower = self
                    .cartridge
                    .source_sample(access.bus_address as usize + 1)
                    .ok_or_else(missing)?;
                Ok(instruction_input::selected(access, upper, lower))
            })
    }
}

#[cfg(test)]
fn reserved_contract() {
    use crate::hardware::bus::scheduled::ScheduledBus;
    use crate::hardware::sound::schedule::SoundSchedule;

    for size in [0x340000, 0x400000] {
        let mut rom = vec![0x69; size];
        rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
        rom[0x256450..0x256452].copy_from_slice(&[0x12, 0x34]);
        rom[0x2c4020..0x2c4022].copy_from_slice(&[0xf3, 0x31]);
        let mut cartridge = Cartridge::new(rom).unwrap();
        cartridge.save = Some(crate::cartridge::SaveMemory::reserved_switched_for_test());
        let mut bus = Bus::new(cartridge, Region::NtscUs);
        let access = |address, lanes| BusAccess {
            logical_address: address,
            bus_address: address & !1,
            lanes,
            kind: AccessKind::Data,
            function_code: FunctionCode::SupervisorData,
        };
        for address in [0x200000, 0x200004, 0x256450, 0x2c4020, size as u32 - 2] {
            let expected = match address {
                0x256450 => 0x1234,
                0x2c4020 => 0xf331,
                _ => 0x6969,
            };
            for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                let logical = address + u32::from(lanes == ByteLanes::Lower);
                let request = access(logical, lanes);
                assert_eq!(bus.read(request), Ok(expected));
                assert_eq!(
                    bus.cartridge.rom_source_offset(logical),
                    Some(logical as usize)
                );
                let before = bus.clone();
                assert!(bus.write(request, 1).is_err());
                assert_eq!(bus, before);
                let write = bus.write_cycle(request, 1, bus.cpu_clock);
                assert!(write.result.is_err());
                assert_eq!(write.wait_clocks, 0);
                assert_eq!(bus, before);
            }
            for kind in [
                AccessKind::Opcode,
                AccessKind::Extension,
                AccessKind::Prefetch,
            ] {
                assert_eq!(
                    bus.read(BusAccess {
                        kind,
                        function_code: FunctionCode::SupervisorProgram,
                        ..access(address, ByteLanes::Both)
                    }),
                    Ok(expected)
                );
            }
        }
        let mut cpu = zeff_z80::Cpu::new();
        let mut schedule = SoundSchedule::default();
        let mut invalid = vec![
            BusAccess {
                bus_address: 0,
                ..access(0x200005, ByteLanes::Lower)
            },
            BusAccess {
                logical_address: 0x200004,
                ..access(0x200005, ByteLanes::Lower)
            },
            BusAccess {
                function_code: FunctionCode::CpuSpace,
                ..access(0x200005, ByteLanes::Lower)
            },
            BusAccess {
                kind: AccessKind::Prefetch,
                ..access(0x200005, ByteLanes::Lower)
            },
            BusAccess {
                kind: AccessKind::Stack,
                ..access(0x200005, ByteLanes::Lower)
            },
        ];
        for address in [0xa13000, 0xa13002, 0xa130f0, 0xa130fe] {
            invalid.extend([
                access(address, ByteLanes::Upper),
                access(address + 1, ByteLanes::Lower),
                access(address, ByteLanes::Both),
            ]);
        }
        if size < 0x400000 {
            for address in [size as u32, 0x3ffffe] {
                invalid.extend([
                    access(address, ByteLanes::Upper),
                    access(address + 1, ByteLanes::Lower),
                    access(address, ByteLanes::Both),
                ]);
                assert_eq!(bus.cartridge.read(address), None);
                assert_eq!(bus.cartridge.rom_source_offset(address), None);
            }
        }
        for request in invalid {
            bus.prefetch = Prefetch::Full(0x1234);
            let before = bus.clone();
            assert!(bus.read(request).is_err());
            assert_eq!(bus, before);
            for value in [0, 1, 2, 3, 4, 0xff] {
                assert!(bus.write(request, value).is_err());
                assert_eq!(bus, before);
                let write = bus.write_cycle(request, value, bus.cpu_clock);
                assert!(write.result.is_err());
                assert_eq!(write.wait_clocks, 0);
                assert_eq!(bus, before);
            }
            let read = bus.read_cycle(request, bus.cpu_clock);
            assert!(read.result.is_err());
            assert_eq!(read.wait_clocks, 0);
            assert_eq!(bus, before);
            let before_cpu = cpu.clone();
            let before_schedule = schedule.clone();
            let clock = bus.cpu_clock;
            let mut scheduled = ScheduledBus::new(&mut bus, &mut cpu, &mut schedule);
            let read = scheduled.read_cycle(request, clock);
            assert!(read.result.is_err());
            assert_eq!(read.wait_clocks, 0);
            for value in [0, 1, 2, 3, 4, 0xff] {
                let write = scheduled.write_cycle(request, value, clock);
                assert!(write.result.is_err());
                assert_eq!(write.wait_clocks, 0);
            }
            assert_eq!(bus, before);
            assert_eq!(cpu, before_cpu);
            assert_eq!(schedule, before_schedule);
        }
        let before = bus.clone();
        let before_cpu = cpu.clone();
        let before_schedule = schedule.clone();
        let clock = bus.cpu_clock;
        let mut scheduled = ScheduledBus::new(&mut bus, &mut cpu, &mut schedule);
        let write = scheduled.write_cycle(access(0x200005, ByteLanes::Lower), 1, clock);
        assert!(write.result.is_err());
        assert_eq!(write.wait_clocks, 0);
        assert_eq!(bus, before);
        assert_eq!(cpu, before_cpu);
        assert_eq!(schedule, before_schedule);
        VdpDmaSource::assert_bounded_odd_reads(&bus.cartridge);
        assert!(bus.cartridge.save.as_ref().unwrap().bytes.is_empty());
    }
}

#[cfg(test)]
pub(super) fn contract() {
    bounded_bytes_contract();
    reserved_contract();
    use crate::hardware::bus::scheduled::ScheduledBus;
    use crate::hardware::sound::schedule::SoundSchedule;

    let mut rom = vec![0x69; 0x280000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    let mut cartridge = Cartridge::new(rom).unwrap();
    cartridge.save = Some(crate::cartridge::SaveMemory::bounded_switched_for_test());
    let mut bus = Bus::new(cartridge, Region::NtscUs);
    let access = |address, lanes| BusAccess {
        logical_address: address,
        bus_address: address & !1,
        lanes,
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    };
    let control = access(0xa130f1, ByteLanes::Lower);
    assert_eq!(bus.read(access(0x200005, ByteLanes::Lower)), Ok(0x6969));
    for kind in [
        AccessKind::Opcode,
        AccessKind::Extension,
        AccessKind::Prefetch,
    ] {
        assert_eq!(
            bus.read(BusAccess {
                kind,
                function_code: FunctionCode::SupervisorProgram,
                ..access(0x200004, ByteLanes::Both)
            }),
            Ok(0x6969)
        );
    }
    let before = bus.clone();
    let rom_write = access(0x200005, ByteLanes::Lower);
    assert!(bus.write(rom_write, 0x47).is_err());
    assert_eq!(bus, before);
    let write = bus.write_cycle(rom_write, 0x47, bus.cpu_clock);
    assert!(write.result.is_err());
    assert_eq!(write.wait_clocks, 0);
    assert_eq!(bus, before);
    bus.write(control, 1).unwrap();
    assert_eq!(bus.read(access(0x200005, ByteLanes::Lower)), Ok(0xff));
    for address in [0x200001, 0x200005, 0x203fff] {
        bus.write(access(address, ByteLanes::Lower), 0x47).unwrap();
        assert_eq!(bus.read(access(address, ByteLanes::Lower)), Ok(0x47));
    }
    bus.write(control, 3).unwrap();
    let cells = bus.cartridge.save.as_ref().unwrap().bytes.clone();
    bus.write(access(0x200005, ByteLanes::Lower), 0x12).unwrap();
    assert_eq!(bus.cartridge.save.as_ref().unwrap().bytes, cells);
    for value in [0, 2] {
        bus.write(control, value).unwrap();
        assert_eq!(bus.read(access(0x200005, ByteLanes::Lower)), Ok(0x6969));
    }
    let mut cpu = zeff_z80::Cpu::new();
    let mut schedule = SoundSchedule::default();
    for mode in [0, 1, 3] {
        bus.write(control, mode).unwrap();
        let mut invalid = vec![
            access(0xa13001, ByteLanes::Lower),
            access(0xa130f0, ByteLanes::Upper),
            access(0xa130f0, ByteLanes::Both),
            access(0xa130f3, ByteLanes::Lower),
            access(0x280001, ByteLanes::Lower),
            access(0x3fffff, ByteLanes::Lower),
            BusAccess {
                bus_address: 0,
                ..access(0x200005, ByteLanes::Lower)
            },
            BusAccess {
                function_code: FunctionCode::CpuSpace,
                ..access(0x200005, ByteLanes::Lower)
            },
            BusAccess {
                logical_address: 0x200004,
                ..access(0x200005, ByteLanes::Lower)
            },
        ];
        if mode != 0 {
            invalid.extend([
                access(0x200004, ByteLanes::Upper),
                access(0x200004, ByteLanes::Both),
                access(0x204001, ByteLanes::Lower),
                BusAccess {
                    kind: AccessKind::Prefetch,
                    ..access(0x200005, ByteLanes::Lower)
                },
            ]);
        }
        for request in invalid {
            bus.prefetch = Prefetch::Full(0x1234);
            let before = bus.clone();
            assert!(bus.read(request).is_err());
            assert_eq!(bus, before);
            assert!(bus.write(request, 0xa5).is_err());
            assert_eq!(bus, before);
            let read = bus.read_cycle(request, bus.cpu_clock);
            assert!(read.result.is_err());
            assert_eq!(read.wait_clocks, 0);
            assert_eq!(bus, before);
            let write = bus.write_cycle(request, 0xa5, bus.cpu_clock);
            assert!(write.result.is_err());
            assert_eq!(write.wait_clocks, 0);
            assert_eq!(bus, before);
            let before_cpu = cpu.clone();
            let before_schedule = schedule.clone();
            let clock = bus.cpu_clock;
            let mut scheduled = ScheduledBus::new(&mut bus, &mut cpu, &mut schedule);
            let read = scheduled.read_cycle(request, clock);
            assert!(read.result.is_err());
            assert_eq!(read.wait_clocks, 0);
            let write = scheduled.write_cycle(request, 0xa5, clock);
            assert!(write.result.is_err());
            assert_eq!(write.wait_clocks, 0);
            assert_eq!(bus, before);
            assert_eq!(cpu, before_cpu);
            assert_eq!(schedule, before_schedule);
        }
        for value in [4, 0x80, 0xff, 0x101] {
            let before = bus.clone();
            assert!(bus.write(control, value).is_err());
            assert_eq!(bus, before);
            let result = bus.write_cycle(control, value, bus.cpu_clock);
            assert!(result.result.is_err());
            assert_eq!(result.wait_clocks, 0);
            assert_eq!(bus, before);
            let before_cpu = cpu.clone();
            let before_schedule = schedule.clone();
            let clock = bus.cpu_clock;
            let mut scheduled = ScheduledBus::new(&mut bus, &mut cpu, &mut schedule);
            let result = scheduled.write_cycle(control, value, clock);
            assert!(result.result.is_err());
            assert_eq!(result.wait_clocks, 0);
            assert_eq!(bus, before);
            assert_eq!(cpu, before_cpu);
            assert_eq!(schedule, before_schedule);
        }
        VdpDmaSource::assert_bounded_odd_reads(&bus.cartridge);
    }
}

#[cfg(test)]
fn bounded_bytes_contract() {
    use crate::hardware::bus::scheduled::ScheduledBus;
    use crate::hardware::sound::schedule::SoundSchedule;
    let request = |address: u32| BusAccess {
        logical_address: address,
        bus_address: address & !1,
        lanes: if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        },
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    };
    for mode in 0..=3 {
        let mut rom = vec![0x69; 0x340000];
        rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
        let mut cartridge = Cartridge::new(rom).unwrap();
        cartridge.save = Some(crate::cartridge::SaveMemory::bounded_bytes_for_test());
        let mut bus = Bus::new(cartridge, Region::NtscUs);
        assert_eq!(bus.write(request(0xa130f1), mode), Ok(()));
        for address in [0x200000, 0x200001, 0x2007fe, 0x2007ff] {
            let access = request(address);
            let shifted = |byte: u8| {
                if address & 1 == 0 {
                    u16::from(byte) << 8
                } else {
                    u16::from(byte)
                }
            };
            if mode & 1 == 0 {
                assert_eq!(bus.read(access), Ok(0x6969));
                let before = bus.clone();
                assert!(bus.write(access, shifted(0x47)).is_err());
                assert_eq!(bus, before);
            } else {
                let before = bus.cartridge.save.as_ref().unwrap().bytes.clone();
                assert_eq!(bus.write(access, shifted(0x47)), Ok(()));
                let expected = if mode == 3 { 0xff } else { 0x47 };
                assert_eq!(bus.read(access), Ok(shifted(expected)));
                if mode == 3 {
                    assert_eq!(bus.cartridge.save.as_ref().unwrap().bytes, before);
                } else {
                    let next = address ^ 1;
                    assert_eq!(
                        bus.cartridge.save.as_ref().unwrap().bytes[(next - 0x200000) as usize],
                        if address & 1 == 0 { 0xff } else { 0x47 }
                    );
                }
            }
        }
        let mut invalid = vec![
            request(0x340000),
            request(0x3fffff),
            request(0xa13001),
            request(0xa130f3),
            BusAccess {
                lanes: ByteLanes::Both,
                ..request(0xa130f0)
            },
            request(0xa130f0),
            BusAccess {
                bus_address: 0,
                ..request(0x200001)
            },
            BusAccess {
                logical_address: 0x200000,
                ..request(0x200001)
            },
            BusAccess {
                function_code: FunctionCode::CpuSpace,
                ..request(0x200001)
            },
        ];
        if mode & 1 != 0 {
            invalid.extend([
                request(0x200800),
                request(0x201000),
                BusAccess {
                    lanes: ByteLanes::Both,
                    ..request(0x200000)
                },
                BusAccess {
                    kind: AccessKind::Prefetch,
                    ..request(0x200000)
                },
                BusAccess {
                    kind: AccessKind::Stack,
                    ..request(0x200001)
                },
            ]);
        }
        for access in invalid {
            bus.prefetch = Prefetch::Full(0x1234);
            let before = bus.clone();
            assert!(bus.read(access).is_err());
            assert!(bus.write(access, 1).is_err());
            assert_eq!(bus, before);
            let result = bus.read_cycle(access, bus.cpu_clock);
            assert!(result.result.is_err());
            assert_eq!(result.wait_clocks, 0);
            let result = bus.write_cycle(access, 1, bus.cpu_clock);
            assert!(result.result.is_err());
            assert_eq!(result.wait_clocks, 0);
            assert_eq!(bus, before);
            let mut cpu = zeff_z80::Cpu::new();
            let mut schedule = SoundSchedule::default();
            let before_cpu = cpu.clone();
            let before_schedule = schedule.clone();
            let clock = bus.cpu_clock;
            let mut scheduled = ScheduledBus::new(&mut bus, &mut cpu, &mut schedule);
            let result = scheduled.read_cycle(access, clock);
            assert!(result.result.is_err());
            assert_eq!(result.wait_clocks, 0);
            let result = scheduled.write_cycle(access, 1, clock);
            assert!(result.result.is_err());
            assert_eq!(result.wait_clocks, 0);
            assert_eq!(bus, before);
            assert_eq!(cpu, before_cpu);
            assert_eq!(schedule, before_schedule);
        }
        for value in [4, 0xff, 0x101] {
            let before = bus.clone();
            assert!(bus.write(request(0xa130f1), value).is_err());
            assert_eq!(bus, before);
        }
        VdpDmaSource::assert_bounded_odd_reads(&bus.cartridge);
    }
}
