use super::*;
use crate::hardware::vdp::VdpDmaSource;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::ClockedBus;

pub(super) fn overlay(machine: &mut Emulator, patches: &[CheatPatch]) {
    assert!(!machine.supports_rom_cheats());
    let ordinary = crate::cartridge::Cartridge::new(machine.bus.cartridge.bytes.to_vec()).unwrap();
    assert!(ordinary.registers.is_none() && ordinary.protection.is_none());
    assert!(ordinary.supports_rom_overlay());
    machine.bus.cartridge.rom_overlay = ordinary.prepare_rom_overlay(patches).unwrap();
}

pub(super) fn refusals(machine: &mut Emulator) {
    assert!(!machine.supports_rom_cheats());
    let before = machine.clone();
    assert!(machine.install_rom_patches(&set(7, 0xa5)).is_err());
    assert!(machine.encode_state().is_err());
    assert!(machine.encode_frontend_state().is_err());
    assert!(machine.encode_debugger_checkpoint().is_err());
    assert!(machine.begin_debug_call(CODE).is_err());
    assert_eq!(*machine, before);
}

pub(super) fn callee(concurrent: bool, banked: bool) {
    let program = if banked {
        vec![0x13fc, 3, 0x0070, 1, 0x4eb9, 1, 0x0100, 0x60fe, 0x4e71]
    } else {
        vec![0x4eb9, 1, 0x0100, 0x60fe, 0x4e71]
    };
    let mut machine = machine(9, concurrent, &program);
    refusals(&mut machine);
    assert_eq!(machine.registers().ssp, 0x00fffffc);
    let a = if banked {
        "010101:6D + 018101:07 + 018106:4E75 + 018000:A5"
    } else {
        "010101:07 + 010106:4E75 + 018000:A5"
    };
    overlay(&mut machine, &patches(a));
    if banked {
        machine.step_instruction().unwrap();
        assert_eq!(machine.peek(CODE + 1), Some(7));
        assert_eq!(machine.peek(0x10000), Some(0xa5));
    }
    machine.step_instruction().unwrap();
    let admitted = machine.cpu.queued_instruction_inputs().unwrap()[0];
    let source = if banked { 0x18100 } else { 0x10100 };
    assert_eq!(admitted.value, 0x7007);
    assert_eq!(offsets(admitted), [Some(source), Some(source + 1)]);
    assert_eq!(machine.rom_offset_for_cpu_address(CODE), None);
    let b = if banked {
        "010101:6D + 018101:09 + 018106:4E75 + 018000:5A"
    } else {
        "010101:09 + 010106:4E75 + 018000:5A"
    };
    overlay(&mut machine, &patches(b));
    let mut capture = Capture::default();
    for _ in 0..3 {
        machine.step_instruction_traced(&mut capture).unwrap();
    }
    assert_eq!(machine.registers().d[0], 10);
    assert_eq!(machine.registers().pc, if banked { 0x20e } else { 0x206 });
    assert_eq!(machine.registers().ssp, 0x00fffffc);
    assert_eq!(capture.0[0], (CODE, admitted));
    assert_eq!(
        offsets(capture.0[3].1),
        [Some(source + 6), Some(source + 7)]
    );
    refusals(&mut machine);
    let mut fresh = super::machine(9, concurrent, &program);
    overlay(&mut fresh, &patches(b));
    let instructions = if banked { 5 } else { 4 };
    for _ in 0..instructions {
        fresh.step_instruction().unwrap();
    }
    assert_eq!(fresh.registers().d[0], 12);
    assert_eq!(fresh.registers().pc, if banked { 0x20e } else { 0x206 });
    for (target, lanes) in [
        (0x700000, ByteLanes::Upper),
        (0x777776, ByteLanes::Both),
        (0x600001, ByteLanes::Lower),
    ] {
        let before = machine.bus.clone();
        let clock = machine.bus.cpu_clock;
        assert!(
            machine
                .bus
                .write_cycle(
                    BusAccess {
                        logical_address: target,
                        bus_address: target & 0xfffffe,
                        function_code: FunctionCode::SupervisorProgram,
                        kind: AccessKind::Prefetch,
                        lanes,
                    },
                    0,
                    clock
                )
                .result
                .is_err()
        );
        assert_eq!(machine.bus, before);
    }
    machine.clear_rom_patches();
    machine.reset();
    assert_eq!(machine.peek(CODE + 1), Some(1));
    assert_eq!(
        machine
            .bus
            .cartridge
            .read_sampled(CODE)
            .unwrap()
            .origin
            .storage_offset(),
        Some(0x10100)
    );
    assert_eq!(machine.registers().pc, 0x200);
}

pub(super) fn queued_main(concurrent: bool) {
    let mut donor = machine(9, concurrent, &[0x4ef9, 1, 0x0100, 0x4e71]);
    overlay(&mut donor, &set(7, 0xa5));
    donor.step_instruction().unwrap();
    let queued = donor.cpu.queued_instruction_inputs().unwrap();
    assert_eq!(queued[0].value, 0x7007);
    assert_eq!(offsets(queued[0]), [Some(0x10100), Some(0x10101)]);
    refusals(&mut donor);
    for current in [Some(9), None] {
        let mut receiver = donor.clone();
        if let Some(value) = current {
            overlay(&mut receiver, &set(value, 0x5a));
        } else {
            receiver.clear_rom_patches();
        }
        assert_eq!(receiver.cpu.queued_instruction_inputs().unwrap(), queued);
        let mut capture = Capture::default();
        receiver.step_instruction_traced(&mut capture).unwrap();
        assert_eq!(capture.0, [(CODE, queued[0])]);
        assert_eq!(receiver.registers().d[0], 7);
        for _ in 0..3 {
            receiver.step_instruction().unwrap();
        }
        assert_eq!(receiver.registers().d[0], u32::from(current.unwrap_or(1)));
        assert_eq!(
            receiver.peek(DATA),
            Some(if current.is_some() { 0x5a } else { 0x12 })
        );
        refusals(&mut receiver);
    }
    overlay(
        &mut donor,
        &patches("000004:8001 + 000006:0100 + 010101:0B"),
    );
    donor.reset();
    assert_eq!(donor.registers().pc, 0x80010100);
    assert_eq!(donor.registers().ssp, 0x00fffffc);
    assert_eq!(donor.queued_words(), Some([0x700b, 0x0640]));
    let mut capture = Capture::default();
    donor.step_instruction_traced(&mut capture).unwrap();
    assert_eq!(capture.0[0].0, 0x80010100);
    assert_eq!(offsets(capture.0[0].1), [Some(0x10100), Some(0x10101)]);
    assert_eq!(donor.registers().d[0], 11);
    donor.clear_rom_patches();
    donor.reset();
    assert_eq!(donor.registers().pc, 0x200);
    assert_eq!(donor.queued_words(), Some([0x4ef9, 1]));
}

pub(super) fn sound() {
    use super::super::sound_tests::{advance_sound, pending_rom_read};
    let mut donor = machine(9, true, &[0x60fe, 0x4e71]);
    donor.bus.sound.set_reset(false);
    donor.bus.sound.bank = 3;
    donor.bus.sound.ram[..7].copy_from_slice(&[0x3a, 0, 0x80, 0x3a, 0, 0x80, 0x76]);
    overlay(&mut donor, &set(7, 0xa5));
    assert!(donor.bus.cartridge.mode_sensitive_read(DATA));
    pending_rom_read(&mut donor);
    let pending = donor.sound_schedule.pending.as_ref().unwrap().clone();
    assert_eq!(pending.cpu.regs().a, 0xa5);
    assert_ne!(donor.sound_cpu.regs().a, 0xa5);
    let deadline = donor.sound_deadline().unwrap().get();
    assert!(donor.bus.now.get() < deadline);
    refusals(&mut donor);
    for current in [Some(0xa5), Some(0x5a), None] {
        let mut receiver = donor.clone();
        if let Some(value) = current {
            overlay(&mut receiver, &set(9, value));
        } else {
            receiver.clear_rom_patches();
        }
        assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&pending));
        assert_eq!(receiver.sound_deadline().unwrap().get(), deadline);
        let remaining = deadline - receiver.bus.now.get();
        advance_sound(&mut receiver, remaining - 1);
        assert_ne!(receiver.sound_cpu.regs().a, 0xa5);
        advance_sound(&mut receiver, 1);
        assert_eq!(receiver.sound_cpu.regs().a, current.unwrap_or(0x12));
        pending_rom_read(&mut receiver);
        let remaining = receiver.sound_deadline().unwrap().get() - receiver.bus.now.get();
        advance_sound(&mut receiver, remaining);
        assert_eq!(receiver.sound_cpu.regs().a, current.unwrap_or(0x12));
    }
}

pub(super) fn dma(mut source: Emulator) {
    use super::super::sound_tests::advance_sound;
    overlay(&mut source, &patches("018000:A55A + 018002:B66B"));
    for _ in 0..8 {
        source.step_instruction().unwrap();
    }
    source
        .bus
        .vdp
        .write_control_from(
            0x0080,
            Region::NtscUs,
            VdpDmaSource::from_cartridge(&source.bus.cartridge, &source.bus.ram),
        )
        .unwrap();
    for _ in 0..2000 {
        if source
            .vdp()
            .fifo()
            .front()
            .is_some_and(|entry| entry.data == 0xa55a)
        {
            break;
        }
        advance_sound(&mut source, 1);
    }
    let first = source.vdp().fifo().front().expect("first sampled DMA word");
    assert_eq!(first.data, 0xa55a);
    assert_ne!(&source.vdp().memory().vram()[0x400..0x402], &[0xa5, 0x5a]);
    refusals(&mut source);
    for current in [
        Some(("018000:A55A + 018002:B66B", [0xb6, 0x6b])),
        Some(("018000:C77C + 018002:D88D", [0xd8, 0x8d])),
        None,
    ] {
        let mut receiver = source.clone();
        if let Some((text, _)) = current {
            overlay(&mut receiver, &patches(text));
        } else {
            receiver.clear_rom_patches();
        }
        assert_eq!(receiver.vdp().fifo().front(), Some(first));
        advance_sound(&mut receiver, 2000);
        assert_eq!(&receiver.vdp().memory().vram()[0x400..0x402], &[0xa5, 0x5a]);
        assert_eq!(
            &receiver.vdp().memory().vram()[0x402..0x404],
            &current.map_or([0x56, 0x78], |(_, bytes)| bytes)
        );
        let counters = crate::VdpDmaRegisters::from_registers(receiver.vdp().registers()).unwrap();
        assert_eq!(
            (counters.length(), counters.source(), counters.source_high()),
            (0, 0x8002, 0)
        );
        assert!(receiver.vdp().fifo().is_empty());
    }
}
