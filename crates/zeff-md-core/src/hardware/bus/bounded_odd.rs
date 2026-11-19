use super::*;
use zeff_m68k::bus::FunctionCode;

impl Bus {
    #[inline]
    pub(super) fn read_bounded_odd(&self, access: BusAccess) -> Option<Result<u16, BusError>> {
        self.cartridge
            .bounded_odd_guarded(access.bus_address)
            .then(|| {
                self.preflight_bounded_odd(access, Direction::Read)?;
                self.cartridge
                    .read(access.bus_address + 1)
                    .map(u16::from)
                    .ok_or(BusError::UnsupportedAccess {
                        address: access.bus_address,
                        direction: Direction::Read,
                    })
            })
    }

    #[inline]
    pub(super) fn preflight_bounded_odd(
        &self,
        access: BusAccess,
        direction: Direction,
    ) -> Result<(), BusError> {
        let logical = access.logical_address & BUS_ADDRESS_MASK;
        if !self.cartridge.bounded_odd_guarded(access.bus_address)
            && ((logical & !1) == access.bus_address
                || !self.cartridge.bounded_odd_guarded(logical))
        {
            return Ok(());
        }
        self.qualify_bounded_odd(access, logical, direction)
    }

    #[cold]
    #[inline(never)]
    fn qualify_bounded_odd(
        &self,
        access: BusAccess,
        logical: u32,
        direction: Direction,
    ) -> Result<(), BusError> {
        let qualified = access.kind == AccessKind::Data
            && matches!(
                access.function_code,
                FunctionCode::UserData | FunctionCode::SupervisorData
            )
            && access.lanes == ByteLanes::Lower
            && access.bus_address & 1 == 0
            && logical == access.bus_address + 1
            && self
                .cartridge
                .save
                .as_ref()
                .is_some_and(|save| save.bounded_odd_byte(logical));
        if qualified {
            Ok(())
        } else {
            Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction,
            })
        }
    }
}

#[cfg(test)]
pub(super) fn contract() {
    use crate::hardware::bus::scheduled::ScheduledBus;
    use crate::hardware::sound::schedule::SoundSchedule;

    let mut rom = vec![0; 0x200];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    let mut cartridge = Cartridge::new(rom).unwrap();
    cartridge.save = Some(crate::cartridge::SaveMemory::bounded_odd_for_test());
    let mut bus = Bus::new(cartridge, Region::NtscUs);
    let access = |address, lanes| BusAccess {
        logical_address: address,
        bus_address: address & !1,
        lanes,
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    };
    for seed in [0x19_u8, 0xe6] {
        for index in 0..2400_u32 {
            let value = seed ^ index as u8 ^ (index >> 8) as u8;
            bus.write(
                access(0x200001 + index * 2, ByteLanes::Lower),
                u16::from(value),
            )
            .unwrap();
        }
        for index in 0..2400_u32 {
            let value = seed ^ index as u8 ^ (index >> 8) as u8;
            assert_eq!(
                bus.read(access(0x200001 + index * 2, ByteLanes::Lower)),
                Ok(u16::from(value))
            );
        }
    }
    assert_eq!(bus.cartridge.read(0x200000), None);
    assert_eq!(bus.cartridge.rom_source_offset(0x200001), None);
    let mut cpu = zeff_z80::Cpu::new();
    let mut schedule = SoundSchedule::default();
    let invalid = [
        access(0x200000, ByteLanes::Upper),
        access(0x200000, ByteLanes::Both),
        access(0x2012c1, ByteLanes::Lower),
        access(0x3fffff, ByteLanes::Lower),
        access(0xa13001, ByteLanes::Lower),
        access(0xa130f1, ByteLanes::Lower),
        BusAccess {
            kind: AccessKind::Prefetch,
            ..access(0x200001, ByteLanes::Lower)
        },
        BusAccess {
            function_code: FunctionCode::CpuSpace,
            ..access(0x200001, ByteLanes::Lower)
        },
        BusAccess {
            logical_address: 0x200000,
            ..access(0x200001, ByteLanes::Lower)
        },
        BusAccess {
            bus_address: 0,
            ..access(0x200001, ByteLanes::Lower)
        },
    ];
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
        assert!(scheduled.read_cycle(request, clock).result.is_err());
        assert!(scheduled.write_cycle(request, 0xa5, clock).result.is_err());
        assert_eq!(bus, before);
        assert_eq!(cpu, before_cpu);
        assert_eq!(schedule, before_schedule);
    }
    VdpDmaSource::assert_bounded_odd_reads(&bus.cartridge);
    let cells = bus.cartridge.save.as_ref().unwrap().bytes.clone();
    bus.cartridge.save.as_mut().unwrap().reset_mapping(0);
    assert_eq!(bus.cartridge.save.as_ref().unwrap().bytes, cells);
}
