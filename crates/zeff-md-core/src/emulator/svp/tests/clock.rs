use super::*;
use crate::svp::FunctionalProfile;
use sha2::{Digest, Sha256};

fn profile() -> FunctionalProfile {
    FunctionalProfile::new(0x418, 1, 3, [1, 1, 2, 2, 1, 1]).unwrap()
}

fn fresh_clocked(sound: bool) -> Emulator {
    let (rom, irom) = images();
    let mut machine = Emulator::new_functional_svp(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: sound,
        },
        irom,
        profile(),
    )
    .unwrap();
    if sound {
        machine.bus.sound.ram[..7].copy_from_slice(&[0x3a, 0, 0x81, 0x32, 0, 1, 0x76]);
        machine.bus.sound.set_reset(false);
    }
    machine
}

fn advance(machine: &mut Emulator, ticks: u64) -> Result<(), BusError> {
    if machine.concurrent_sound {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .advance_machine(ticks)
    } else {
        machine.bus.advance_machine(ticks)
    }
}

fn malformed(machine: &Emulator) {
    let model = machine.synthetic_svp().unwrap();
    let bytes = model.encode_state().unwrap();
    let at = bytes
        .windows(8)
        .position(|part| part == b"ZSVPFNC1")
        .unwrap();
    let mut target = fresh_clocked(machine.concurrent_sound);
    let before = target.encode_state().unwrap();
    for offset in [0, 8, 16, 20, 24, 36, 44, 52, 56, 65] {
        let mut corrupt = bytes.clone();
        corrupt[at + offset] ^= 0xff;
        assert!(
            target
                .bus
                .cartridge
                .svp
                .as_mut()
                .unwrap()
                .decode_state(&corrupt)
                .is_err(),
            "clock offset {offset}"
        );
        assert_eq!(target.encode_state().unwrap(), before);
    }
    let (rom, irom) = images();
    let mut different = Emulator::new_functional_svp(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: false,
        },
        irom,
        FunctionalProfile::new(0x419, 1, 3, [1, 1, 2, 2, 1, 1]).unwrap(),
    )
    .unwrap();
    assert!(
        different
            .bus
            .cartridge
            .svp
            .as_mut()
            .unwrap()
            .decode_state(&bytes)
            .is_err()
    );
}

pub(super) fn contract() {
    assert!(FunctionalProfile::new(0, 1, 1, [1; 6]).is_err());
    assert!(FunctionalProfile::new(1, 2, 1, [1; 6]).is_err());
    assert!(FunctionalProfile::new(1, 1, 1, [0; 6]).is_err());
    let mut digest = Sha256::new();
    for sound in [false, true] {
        let mut whole = fresh_clocked(sound);
        let mut split = whole.clone();
        advance(&mut whole, 997).unwrap();
        for ticks in [1, 2, 17, 131, 256, 590] {
            advance(&mut split, ticks).unwrap();
        }
        assert_eq!(whole.encode_state().unwrap(), split.encode_state().unwrap());
        if sound {
            assert_eq!(whole.bus.sound.ram[0x100], 0x33);
        }
        malformed(&whole);
        assert!(whole.advance_synthetic_svp_service(true).is_err());
        assert!(whole.advance_synthetic_svp(true).is_err());
        let bytes = whole.encode_state().unwrap();
        let mut restored = fresh_clocked(sound);
        restored.decode_state(&bytes).unwrap();
        for _ in 0..32 {
            whole.step_instruction().unwrap();
            restored.step_instruction().unwrap();
            let bytes = whole.encode_state().unwrap();
            assert_eq!(bytes, restored.encode_state().unwrap());
            digest.update(&bytes);
            assert_eq!(
                whole
                    .synthetic_svp()
                    .unwrap()
                    .functional_timing()
                    .unwrap()
                    .master_tick,
                whole.bus.now.get()
            );
        }
        for _ in 0..64 {
            whole.step_instruction().unwrap();
        }
        assert_eq!(whole.cpu.registers.d[4] & 0xffff, 0xabcd);
        assert!(
            whole
                .synthetic_svp()
                .unwrap()
                .registers()
                .committed_instructions
                > 0
        );
        let clock = whole.synthetic_svp().unwrap().functional_timing().unwrap();
        whole.reset();
        assert_eq!(
            whole
                .synthetic_svp()
                .unwrap()
                .functional_timing()
                .unwrap()
                .profile,
            clock.profile
        );
        assert_eq!(
            whole
                .synthetic_svp()
                .unwrap()
                .functional_timing()
                .unwrap()
                .master_tick,
            whole.bus.now.get()
        );
        hold_and_pause(sound, &mut digest);
        rollback(sound);
    }
    failed_restart();
    late_payload_rollback();
    frontier_fault_priority();
    stopped(&mut digest);
    active_sound(&mut digest);
    dma_tie(&mut digest);
    let hash: String = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let message = format!("SVP418_CLOCK={hash}/64");
    #[cfg(not(target_arch = "wasm32"))]
    println!("{message}");
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("{message}");
}

fn write(
    machine: &mut Emulator,
    address: u32,
    value: u16,
) -> zeff_m68k::timing::CycleResult<(), BusError> {
    let access = access(address, ByteLanes::Both);
    let start = machine.bus.cpu_clock;
    let result = if machine.concurrent_sound {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .write_cycle(access, value, start)
    } else {
        machine.bus.write_cycle(access, value, start)
    };
    if result.result.is_ok() {
        machine.cpu = zeff_m68k::timing::PrefetchCpu::from_prefetched(
            machine.cpu.registers,
            machine.cpu.queued_words().unwrap(),
            machine.bus.cpu_clock,
        )
        .unwrap();
    }
    result
}

fn hold_and_pause(sound: bool, digest: &mut Sha256) {
    let mut machine = fresh_clocked(sound);
    advance(&mut machine, 7).unwrap();
    let begin = machine.bus.now.get();
    let start = machine.bus.cpu_clock.0;
    let result = write(&mut machine, 0xa15006, 0xa);
    result.result.unwrap();
    assert_eq!(
        machine.bus.cpu_clock.0 - start,
        u64::from(result.wait_clocks) + 4
    );
    assert_eq!(
        machine.bus.now.get() - begin,
        (u64::from(result.wait_clocks) + 4) * MAIN_CPU_DIVISOR
    );
    let halted = machine.synthetic_svp().unwrap().registers();
    let saved = machine.encode_state().unwrap();
    let mut split = fresh_clocked(sound);
    split.decode_state(&saved).unwrap();
    advance(&mut machine, 1001).unwrap();
    for ticks in [1, 17, 983] {
        advance(&mut split, ticks).unwrap();
    }
    assert_eq!(machine.synthetic_svp().unwrap().registers(), halted);
    assert_eq!(
        machine.encode_state().unwrap(),
        split.encode_state().unwrap()
    );
    let bytes = machine.encode_state().unwrap();
    assert!(write(&mut machine, 0xa15006, 1).result.is_err());
    assert_eq!(machine.encode_state().unwrap(), bytes);
    write(&mut machine, 0xa15006, 0).result.unwrap();
    advance(&mut machine, 100).unwrap();
    digest.update(machine.encode_state().unwrap());
    let at = machine.bus.now.get();
    machine
        .bus
        .cartridge
        .svp
        .as_mut()
        .unwrap()
        .advance_functional_to(at + 3)
        .unwrap();
    assert!(machine.encode_state().is_err());
}

fn bad_machine(sound: bool, code: u16, offset: usize) -> Emulator {
    let (rom, mut irom) = images();
    irom[8..100].fill(0);
    irom[8 + offset] = code;
    Emulator::new_functional_svp(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: sound,
        },
        irom,
        FunctionalProfile::new(0xbad418, 1, 1, [1; 6]).unwrap(),
    )
    .unwrap()
}

fn rollback(sound: bool) {
    let mut machine = bad_machine(sound, 0xffff, 0);
    let before = machine.encode_state().unwrap();
    assert!(advance(&mut machine, 28).is_err());
    assert_eq!(machine.encode_state().unwrap(), before);
    let mut machine = bad_machine(sound, 0x00d8, 0);
    advance(&mut machine, 2).unwrap();
    let before = machine.encode_state().unwrap();
    assert!(write(&mut machine, 0xa15000, 0x1234).result.is_err());
    assert_eq!(machine.encode_state().unwrap(), before);
}

fn failed_restart() {
    let mut machine = bad_machine(false, 0xffff, 30);
    let before = machine.encode_state().unwrap();
    assert!(machine.restart_cpu().is_err());
    assert_eq!(machine.encode_state().unwrap(), before);
}

fn late_payload_rollback() {
    let (rom, mut irom) = images();
    irom[8..408].fill(0);
    irom[408] = 0xffff;
    let mut machine = Emulator::new_functional_svp(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: false,
        },
        irom,
        FunctionalProfile::new(0x419, 1, 11, [1; 6]).unwrap(),
    )
    .unwrap();
    for word in [0x8114, 0x8f02, 0x9301, 0x9400, 0x9500, 0x9600, 0x9700] {
        machine.bus.vdp.write_control(word, Region::NtscUs).unwrap();
        if let Some(pending) = machine.bus.vdp.registers().pending() {
            let ticks = pending.publish_at.get() - machine.bus.now.get();
            advance(&mut machine, ticks).unwrap();
        }
    }
    machine
        .bus
        .vdp
        .write_control(0x4000, Region::NtscUs)
        .unwrap();
    machine
        .bus
        .vdp
        .write_control(0x0080, Region::NtscUs)
        .unwrap();
    let before = machine.encode_state().unwrap();
    let vram_pointer = machine.bus.vdp.memory().vram().as_ptr();
    let pixels_pointer = machine.framebuffer().as_ptr();
    let mut prefix = machine.clone();
    advance(&mut prefix, 3600).unwrap();
    assert_ne!(
        prefix.bus.vdp.memory().vram(),
        machine.bus.vdp.memory().vram()
    );
    assert_ne!(prefix.framebuffer(), machine.framebuffer());
    assert!(advance(&mut machine, 5000).is_err());
    assert_eq!(machine.encode_state().unwrap(), before);
    assert_eq!(machine.bus.vdp.memory().vram().as_ptr(), vram_pointer);
    assert_eq!(machine.framebuffer().as_ptr(), pixels_pointer);
}

fn frontier_fault_priority() {
    let (rom, mut irom) = images();
    irom[8] = 0xffff;
    let mut machine = Emulator::new_functional_svp(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: false,
        },
        irom,
        FunctionalProfile::new(0x423, 1, 4096, [1; 6]).unwrap(),
    )
    .unwrap();
    advance(&mut machine, 4096).unwrap();
    for word in [0x8114, 0x8f02, 0x9301, 0x9400, 0x9500, 0x9600, 0x9710] {
        machine.bus.vdp.write_control(word, Region::NtscUs).unwrap();
        if let Some(pending) = machine.bus.vdp.registers().pending() {
            let ticks = pending.publish_at.get() - machine.bus.now.get();
            advance(&mut machine, ticks).unwrap();
        }
    }
    machine
        .bus
        .vdp
        .write_control(0x4000, Region::NtscUs)
        .unwrap();
    machine
        .bus
        .vdp
        .write_control(0x0080, Region::NtscUs)
        .unwrap();
    let now = machine.bus.now.get();
    let end = machine
        .synthetic_svp()
        .unwrap()
        .functional_event_tick()
        .unwrap()
        .unwrap();
    // Locate the first fetch on the same schedule using an unpopulated ROM.
    let fetch = machine
        .bus
        .vdp
        .external_dma_release(Region::NtscUs, crate::VdpDmaSource::new(&[], &[]))
        .unwrap()
        .unwrap()
        .get();
    assert!(now < fetch && fetch < end);
    let cause = crate::svp::Error::from(crate::svp::ssp1601::Fault::Opcode(0xffff));
    let mut dsp = machine.synthetic_svp().unwrap().clone();
    assert_eq!(dsp.advance_functional_to(end), Err(cause.clone()));
    let vdp_error = crate::VdpScheduleError::Dma(crate::VdpDmaError::UnsupportedSource(0x200000));
    let mut vdp = machine.bus.vdp.clone();
    let source = crate::VdpDmaSource::from_cartridge(&machine.bus.cartridge, &machine.bus.ram);
    assert_eq!(
        vdp.advance_with_source(MasterTicks::new(end), Region::NtscUs, source),
        Err(vdp_error)
    );
    let before = machine.encode_state().unwrap();
    let mut earlier = machine.clone();
    assert_eq!(
        advance(&mut earlier, fetch - now),
        Err(BusError::Vdp(vdp_error))
    );
    assert_eq!(earlier.encode_state().unwrap(), before);
    assert_eq!(
        advance(&mut machine, end - now),
        Err(BusError::Owned(format!("functional SVP: {cause}"))),
    );
    assert_eq!(machine.encode_state().unwrap(), before);
}

fn stopped(digest: &mut Sha256) {
    let (mut rom, irom) = images();
    rom[0x100..0x104].copy_from_slice(&[0x4e, 0x72, 0x27, 0]);
    let mut machine = Emulator::new_functional_svp(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: false,
        },
        irom,
        profile(),
    )
    .unwrap();
    machine.step_instruction().unwrap();
    assert!(machine.cpu.stopped());
    let before = machine
        .synthetic_svp()
        .unwrap()
        .functional_timing()
        .unwrap();
    machine.step_instruction().unwrap();
    let after = machine
        .synthetic_svp()
        .unwrap()
        .functional_timing()
        .unwrap();
    assert!(
        after.master_tick > before.master_tick && after.completed_grants > before.completed_grants
    );
    digest.update(machine.encode_state().unwrap());
}

fn active_sound(digest: &mut Sha256) {
    let mut machine = fresh_clocked(true);
    let mut captured = false;
    for _ in 0..256 {
        advance(&mut machine, 1).unwrap();
        if machine
            .sound_schedule
            .pending
            .as_ref()
            .is_some_and(|pending| {
                pending
                    .bank_access
                    .is_some_and(|access| access.address == 0x100)
            })
        {
            captured = true;
            break;
        }
    }
    assert!(captured);
    assert!(
        machine
            .synthetic_svp()
            .unwrap()
            .functional_timing()
            .unwrap()
            .completed_grants
            > 0
    );
    let mut copy = fresh_clocked(true);
    copy.decode_state(&machine.encode_state().unwrap()).unwrap();
    advance(&mut machine, 600).unwrap();
    advance(&mut copy, 600).unwrap();
    assert_eq!(machine.bus.sound.ram[0x100], 0x33);
    assert_eq!(
        machine.encode_state().unwrap(),
        copy.encode_state().unwrap()
    );
    digest.update(machine.encode_state().unwrap());
}

fn dma_tie(digest: &mut Sha256) {
    let initial = dma_candidate(1);
    let delay = dma_release(&initial).unwrap() - initial.bus.now.get();
    let mut machine = dma_candidate(u16::try_from(delay).unwrap());
    let event = machine
        .synthetic_svp()
        .unwrap()
        .functional_event_tick()
        .unwrap()
        .unwrap();
    assert_eq!(dma_release(&machine), Some(event));
    let mut split = machine.clone();
    advance(&mut machine, delay + 64).unwrap();
    advance(&mut split, delay - 1).unwrap();
    advance(&mut split, 1).unwrap();
    advance(&mut split, 64).unwrap();
    assert_eq!(
        machine.encode_state().unwrap(),
        split.encode_state().unwrap()
    );
    assert!(dma_release(&machine).is_none());
    assert_eq!(machine.synthetic_svp().unwrap().peek_dram(0), 0x1235);
    assert_eq!(&machine.bus.vdp.memory().vram()[..2], &[0, 0]);
    digest.update(machine.encode_state().unwrap());
}

fn dma_candidate(dram_cost: u16) -> Emulator {
    use crate::svp::{ServiceResource, ServiceRole};
    let (rom, irom) = images();
    let mut machine = Emulator::new_functional_svp(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: false,
        },
        irom,
        FunctionalProfile::new(0xd418, 1, 1, [1, 1, 1, dram_cost, 1, 1]).unwrap(),
    )
    .unwrap();
    write(&mut machine, 0xa15000, 0x1234).result.unwrap();
    write(&mut machine, 0xa15006, 0xa).result.unwrap();
    for word in [0x8114, 0x8f02, 0x9301, 0x9400, 0x9500, 0x9600, 0x9718] {
        machine.bus.vdp.write_control(word, Region::NtscUs).unwrap();
        if let Some(pending) = machine.bus.vdp.registers().pending() {
            let delay = pending.publish_at.get() - machine.bus.now.get();
            advance(&mut machine, delay).unwrap();
        }
    }
    machine
        .bus
        .vdp
        .write_control(0x4000, Region::NtscUs)
        .unwrap();
    write(&mut machine, 0xa15006, 0).result.unwrap();
    let mut pending = false;
    for _ in 0..4096 {
        if machine
            .synthetic_svp()
            .unwrap()
            .pending_service()
            .unwrap()
            .is_some_and(|request| {
                request.role == ServiceRole::PortWrite(0)
                    && request.resource == ServiceResource::SharedDram
                    && request.address == 0x180000
            })
        {
            pending = true;
            break;
        }
        advance(&mut machine, 1).unwrap();
    }
    assert!(pending);
    machine
        .bus
        .vdp
        .write_control(0x0080, Region::NtscUs)
        .unwrap();
    machine
}

fn dma_release(machine: &Emulator) -> Option<u64> {
    let source = crate::VdpDmaSource::from_cartridge(&machine.bus.cartridge, &machine.bus.ram);
    machine
        .bus
        .vdp
        .external_dma_release(machine.bus.region, source)
        .unwrap()
        .map(|tick| tick.get())
}
