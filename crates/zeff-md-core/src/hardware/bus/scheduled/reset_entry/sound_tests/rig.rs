use super::*;
use crate::cartridge::registers::CartridgeRegisters;
use zeff_m68k::Registers;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Rig {
    pub bus: Bus,
    pub sound: Cpu,
    pub schedule: SoundSchedule,
}

impl Rig {
    pub fn new(
        program: &[u8],
        cut: u64,
        epoch: bool,
        batched: bool,
        bank: u16,
        registers: bool,
    ) -> Self {
        let mut image = vec![0; 0x200];
        image[..8].copy_from_slice(&[0, 0xff, 0x80, 0, 0, 0, 1, 0]);
        for word in image[0x100..].as_chunks_mut::<2>().0 {
            word.copy_from_slice(&0x4e71u16.to_be_bytes());
        }
        let mut bus = Bus::new(Cartridge::new(image).unwrap(), Region::NtscUs);
        if registers {
            bus.cartridge.registers = Some(CartridgeRegisters::default());
            bus.cartridge.registers.as_mut().unwrap().write(0, 0xde);
        }
        bus.ram[0x8000] = 0xde;
        bus.sound.set_reset(false);
        bus.sound.bank = bank;
        bus.sound.ram[..program.len()].copy_from_slice(program);
        bus.sound.ram[0x100] = 0xde;
        bus.audio.write_psg(0, 0x84);
        bus.audio.write_psg(0, 1);
        bus.audio.write_psg(0, 0x90);
        if batched {
            bus.audio.begin_batch();
        }
        let mut result = Self {
            bus,
            sound: Cpu::new(),
            schedule: SoundSchedule::default(),
        };
        if epoch {
            result.schedule.begin_epoch_field();
        }
        result.schedule.advance(
            &mut result.sound,
            &mut result.bus.sound,
            &mut result.bus.cartridge,
            &mut result.bus.ram,
            SoundInterval::new(0..cut, &mut result.bus.audio),
        );
        assert_eq!(result.schedule.fault, None);
        result.bus.advance_machine(cut).unwrap();
        result.bus.cpu_clock = CpuClocks(23);
        result
    }

    pub fn main(&self) -> PrefetchCpu {
        let mut registers = Registers::new();
        registers.pc = 0x100;
        registers.ssp = 0xff1000;
        registers.usp = 0x12345678;
        registers.d[0] = 0xa5ff1234;
        registers.set_status_register(0x2115);
        PrefetchCpu::from_prefetched(registers, [0x4e71; 2], self.bus.cpu_clock).unwrap()
    }

    pub fn scheduled(&mut self) -> ScheduledBus<'_> {
        ScheduledBus::new(&mut self.bus, &mut self.sound, &mut self.schedule)
    }

    pub fn target(&mut self, concurrent: bool) -> Target<'_> {
        if concurrent {
            Target::Scheduled(self.scheduled())
        } else {
            Target::Direct(&mut self.bus)
        }
    }

    pub fn close_epoch(&mut self) {
        self.schedule.finish_epoch_field(
            &mut self.sound,
            &self.bus.sound,
            &mut crate::sound_trace::NoSoundCapture,
        );
    }

    pub fn finish_batch(&mut self, batched: bool) {
        if batched {
            self.bus.audio.finish_batch();
            self.bus.audio.begin_batch();
            self.bus.audio.finish_batch();
        }
    }

    pub fn value(&self, kind: u8) -> u8 {
        match kind {
            0 => self.bus.sound.ram[0x100],
            1 => self.bus.ram[0x8000],
            2 => self
                .bus
                .cartridge
                .registers
                .as_ref()
                .unwrap()
                .read(0x400000)
                .unwrap(),
            _ => unreachable!(),
        }
    }

    pub fn replace_value(&mut self, kind: u8) {
        match kind {
            0 => self.bus.sound.ram[0x100] = 0xa6,
            1 => self.bus.ram[0x8000] = 0xa6,
            2 => self
                .bus
                .cartridge
                .registers
                .as_mut()
                .unwrap()
                .write(0, 0xa6),
            _ => unreachable!(),
        }
    }

    pub fn replay(&mut self, concurrent: bool, events: &[Event]) {
        fn apply(bus: &mut impl ClockedBus<Error = BusError>, events: &[Event]) {
            for event in events {
                match event {
                    Event::Idle(start, clocks) => bus.idle(*start, *clocks),
                    Event::Read(access, start, report) => {
                        let actual = bus.read_cycle(*access, *start);
                        assert_eq!(report.elapsed, 4 + actual.wait_clocks);
                        assert_eq!(
                            report.outcome,
                            EntryOutcome::Complete(actual.result.unwrap())
                        );
                    }
                }
            }
        }
        if concurrent {
            apply(&mut self.scheduled(), events)
        } else {
            apply(&mut self.bus, events)
        }
    }

    pub fn advance(&mut self, concurrent: bool, ticks: u64) {
        if concurrent {
            self.scheduled().advance_machine(ticks).unwrap()
        } else {
            self.bus.advance_machine(ticks).unwrap()
        }
    }

    pub fn pcm(&mut self) -> Vec<u32> {
        let mut pcm = vec![];
        self.bus.audio.drain_into(&mut pcm);
        pcm.into_iter().map(f32::to_bits).collect()
    }
}
