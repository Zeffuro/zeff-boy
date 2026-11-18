use super::*;
use zeff_m68k::bus::{BusAccess, ByteLanes};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu};

struct PortBus {
    vdp: Vdp,
    region: Region,
    origin: u64,
    writes: Vec<u64>,
}

impl PortBus {
    fn advance(&mut self, start: CpuClocks, clocks: u32) {
        self.vdp
            .advance_to(
                tick(self.origin + (start.0 + u64::from(clocks)) * 7),
                self.region,
            )
            .unwrap();
    }
}

impl ClockedBus for PortBus {
    type Error = std::convert::Infallible;

    fn read_cycle(
        &mut self,
        _access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<u16, Self::Error> {
        self.advance(start, 4);
        CycleResult::ready(0x4e71)
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), Self::Error> {
        self.advance(start, 4);
        assert_eq!(access.bus_address, 0xc00004);
        assert_eq!(access.lanes, ByteLanes::Both);
        self.writes.push(self.vdp.now().get());
        self.vdp.write_control(value, self.region).unwrap();
        CycleResult::ready(())
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.advance(start, clocks);
    }
}

#[test]
fn actual_68000_control_writes_match_all_eight_recorded_timing_conditions() {
    for (region, at, id, row, phase, boundary, image) in [
        (
            Region::NtscJapan,
            32268250,
            36,
            3,
            550,
            32271120,
            [
                4, 52, 40, 48, 5, 110, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0,
            ],
        ),
        (
            Region::NtscUs,
            15233526,
            17,
            0,
            846,
            15236100,
            [
                20, 52, 48, 56, 7, 120, 0, 0, 0, 0, 0, 0, 0, 61, 0, 2, 1, 0, 0, 0, 0, 0, 0, 128,
            ],
        ),
        (
            Region::NtscUs,
            3665984,
            4,
            23,
            3164,
            3666240,
            [
                4, 116, 56, 52, 6, 122, 0, 0, 0, 0, 0, 0, 129, 0, 0, 2, 1, 0, 0, 0, 0, 239, 181, 2,
            ],
        ),
        (
            Region::NtscUs,
            52592232,
            58,
            181,
            2892,
            52592760,
            [
                4, 52, 40, 48, 5, 110, 0, 0, 0, 0, 0, 1, 0, 0, 0, 2, 3, 0, 0, 0, 0, 0, 0, 0,
            ],
        ),
        (
            Region::NtscUs,
            587735148,
            655,
            242,
            1308,
            587737260,
            [
                20, 36, 8, 60, 0, 10, 0, 0, 0, 0, 230, 0, 129, 32, 0, 2, 17, 0, 0, 0, 0, 255, 255,
                128,
            ],
        ),
        (
            Region::NtscUs,
            143442817,
            160,
            22,
            1177,
            143445060,
            [
                20, 36, 8, 60, 0, 10, 0, 0, 0, 0, 230, 0, 129, 32, 0, 2, 17, 0, 0, 0, 0, 255, 255,
                128,
            ],
        ),
        (
            Region::NtscUs,
            4088,
            0,
            1,
            668,
            6840,
            [
                0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
        ),
        (
            Region::NtscUs,
            2524914,
            2,
            214,
            954,
            2527380,
            [
                4, 52, 40, 48, 5, 110, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0,
            ],
        ),
    ] {
        let registers = VdpRegisterPipeline::clock_fixture(tick(0), tick(0), image);
        let interrupts = VdpInterrupts::at_counters_zero(tick(0), image[10], false, false);
        let mut vdp = Vdp::from_stable_display_state(interrupts, registers, region).unwrap();
        let origin = at - 28;
        vdp.advance_to(tick(origin), region).unwrap();
        let mut bus = PortBus {
            vdp,
            region,
            origin,
            writes: Vec::new(),
        };
        let mut registers = zeff_m68k::Registers::default();
        registers.pc = 0x100;
        registers.a[0] = 0xc00004;
        registers.d[0] = 0x8c01;
        let mut cpu =
            PrefetchCpu::from_prefetched(registers, [0x3080, 0x4e71], CpuClocks(0)).unwrap();
        assert_eq!(cpu.step_instruction(&mut bus).unwrap().clocks, 8);
        assert_eq!(bus.writes, [at]);
        let position = bus.vdp.field_position(region).unwrap();
        assert_eq!((position.id, u64::from(position.line)), (id, row));
        assert_eq!(at - position.line_start.get(), phase);
        assert_eq!(bus.vdp.registers().registers()[12], Some(1));
        assert_eq!(bus.vdp.effective_line_ticks(), 3420);
        let completed = bus.vdp.completed_row(tick(boundary), region).unwrap();
        if row < 224 {
            assert_eq!(
                completed.map(|(line, field)| (u64::from(line), field.id)),
                Some((row, id))
            );
        }
        let mut restored = restore(&bus.vdp, region);
        for now in [boundary - 1, boundary, boundary + 3359, boundary + 3360] {
            bus.vdp.advance_to(tick(now), region).unwrap();
            restored.advance_to(tick(now), region).unwrap();
            assert_eq!(restored, bus.vdp);
        }
        assert_eq!(bus.vdp.effective_line_ticks(), 3360);
        assert_eq!(
            bus.vdp.next_row_deadline(region).unwrap(),
            Some(tick(boundary + 6720))
        );
    }
}
