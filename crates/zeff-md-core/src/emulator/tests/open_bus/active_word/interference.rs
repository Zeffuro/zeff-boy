use super::*;
use zeff_m68k::timing::CycleResult;

fn cycle(machine: &mut Emulator, request: BusAccess) -> CycleResult<u16, BusError> {
    let clock = machine.bus.cpu_clock;
    ScheduledBus::new(
        &mut machine.bus,
        &mut machine.sound_cpu,
        &mut machine.sound_schedule,
    )
    .read_cycle(request, clock)
}

fn unsupported(address: u32) -> Result<u16, BusError> {
    Err(BusError::UnsupportedAccess {
        address,
        direction: Direction::Read,
    })
}

fn interference_requests() -> [(u32, ByteLanes); 4] {
    [
        (SOUND_BUS_REQUEST, ByteLanes::Both),
        (SOUND_BUS_REQUEST, ByteLanes::Upper),
        (SOUND_BUS_REQUEST, ByteLanes::Lower),
        (crate::constants::IO_UNUSED_END, ByteLanes::Both),
    ]
}

fn source(machine: &mut Emulator, word: u16) {
    machine.bus.ram[0x100..0x102].copy_from_slice(&word.to_be_bytes());
    assert_eq!(
        cycle(
            machine,
            access(0xff0100, AccessKind::Prefetch, ByteLanes::Both)
        )
        .result,
        Ok(word)
    );
    assert_eq!(machine.bus.prefetch, Prefetch::Full(word));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn busreq_active_word_incomplete_history_lanes_and_consumption() {
    let mut hash = Sha256::new();
    let mut cases = 0;
    for phase in [
        Phase::Crossing,
        Phase::Pending,
        Phase::Granted,
        Phase::Reset,
    ] {
        for tag in [
            Prefetch::Empty,
            Prefetch::UpperOnly(0xa5),
            Prefetch::Full(0xa55a),
        ] {
            for (address, lanes) in interference_requests() {
                let mut machine = fixture(Region::NtscUs, phase, 1);
                machine.bus.prefetch = tag;
                let request = access(address, AccessKind::Data, lanes);
                let result = cycle(&mut machine, request);
                assert_eq!(result.wait_clocks, 0);
                let granted = machine.bus.sound.granted;
                let status = u16::from(!granted) << 8;
                let active_at_entry = matches!(phase, Phase::Crossing | Phase::Pending);
                let expected = match (address, lanes, tag) {
                    (crate::constants::IO_UNUSED_END, ByteLanes::Both, Prefetch::Full(word)) => {
                        Ok(word)
                    }
                    (crate::constants::IO_UNUSED_END, _, _) => unsupported(address),
                    (_, ByteLanes::Both, Prefetch::Full(word)) => Ok((word & 0xfeff) | status),
                    (_, ByteLanes::Lower, Prefetch::Full(word)) => Ok(word & 0xff),
                    (_, ByteLanes::Upper, _) if active_at_entry => Ok(status),
                    (_, ByteLanes::Upper, _) => {
                        Ok((u16::from(tag.upper().unwrap_or_default()) << 8 & 0xfeff) | status)
                    }
                    _ => unsupported(address),
                };
                assert_eq!(result.result, expected, "{phase:?} {tag:?} {lanes:?}");
                assert_eq!(machine.bus.prefetch, Prefetch::Empty);
                let repeated = cycle(&mut machine, request);
                assert_eq!(
                    repeated.result,
                    if lanes == ByteLanes::Upper {
                        Ok(u16::from(!machine.bus.sound.granted) << 8)
                    } else {
                        unsupported(address)
                    }
                );
                hash.update(format!("{phase:?}{tag:?}{lanes:?}{result:?}{repeated:?}").as_bytes());
                cases += 1;
            }
        }
    }
    emit("busreq-active-lanes-v1", hash, cases);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn busreq_active_word_external_dma_invalidates_real_history() {
    let mut hash = Sha256::new();
    for (address, lanes) in interference_requests() {
        let mut machine = super::super::super::machine(&[0x4e71; 2]);
        super::super::super::vdp_dma::configure(&mut machine, 17, 0, 2);
        machine.bus.advance_machine(1).unwrap();
        let write = machine
            .bus
            .vdp
            .queue_register(1, 0x14, Region::NtscUs)
            .unwrap();
        machine
            .bus
            .advance_machine(write.publish_at.get() - machine.bus.now.get() + 1)
            .unwrap();
        source(&mut machine, 0xa55a);
        machine
            .bus
            .vdp
            .write_control(0x4000, Region::NtscUs)
            .unwrap();
        machine.bus.vdp.write_control(0x80, Region::NtscUs).unwrap();
        let result = cycle(&mut machine, access(address, AccessKind::Data, lanes));
        assert!(result.wait_clocks > 0);
        assert_eq!(
            result.result,
            if lanes == ByteLanes::Upper {
                Ok(0x100)
            } else {
                unsupported(address)
            }
        );
        assert_eq!(machine.bus.prefetch, Prefetch::Empty);
        hash.update(
            format!(
                "{address:x}{lanes:?}{result:?}{:?}",
                machine.timing_snapshot()
            )
            .as_bytes(),
        );
    }
    emit("busreq-active-dma-v1", hash, 4);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn busreq_active_word_sound_reservation_wait_invalidates_real_history() {
    let mut hash = Sha256::new();
    for (address, lanes) in interference_requests() {
        let mut machine = super::super::super::bank_reservation::ready(&[0x36, 0x5a, 0x76]);
        let clock = machine.bus.cpu_clock;
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .idle(clock, TRANSFER_CLOCKS);
        let pending = machine.sound_schedule.pending.as_ref().unwrap();
        assert!(pending.any_bank_access);
        assert!(pending.reservation().is_some());
        let end = pending.deadline;
        machine.bus.sound.request(true);
        machine.bus.sound.complete_boundary();
        assert!(machine.bus.sound.granted);
        machine.bus.ram[0x100..0x102].copy_from_slice(&0xa55a_u16.to_be_bytes());
        let clock = machine.bus.cpu_clock;
        assert_eq!(
            machine
                .bus
                .read_cycle(
                    access(0xff0100, AccessKind::Prefetch, ByteLanes::Both),
                    clock
                )
                .result,
            Ok(0xa55a)
        );
        assert_eq!(machine.bus.prefetch, Prefetch::Full(0xa55a));
        let result = cycle(&mut machine, access(address, AccessKind::Data, lanes));
        assert!(result.wait_clocks > 0);
        assert!(machine.bus.now.get() >= end + u64::from(TRANSFER_CLOCKS) * MAIN_CPU_DIVISOR);
        assert_eq!(
            result.result,
            if lanes == ByteLanes::Upper {
                Ok(0)
            } else {
                unsupported(address)
            }
        );
        assert_eq!(machine.bus.prefetch, Prefetch::Empty);
        hash.update(
            format!(
                "{address:x}{lanes:?}{result:?}{:?}",
                machine.timing_snapshot()
            )
            .as_bytes(),
        );
    }
    emit("busreq-active-reservation-v1", hash, 4);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn busreq_active_word_banked_candidate_without_reservation_invalidates_history() {
    use zeff_emu_common::save_state::{StateReader, StateWriter};
    let mut hash = Sha256::new();
    for (address, lanes) in interference_requests() {
        let mut machine = super::super::super::machine(&[0x4e71; 2]);
        machine.bus.sound.set_reset(false);
        machine.bus.sound.ram[..4].copy_from_slice(&[0x3a, 0, 0x80, 0x76]);
        let pending = PendingSound::prepare(
            &machine.sound_cpu,
            &machine.bus.sound,
            &machine.bus.cartridge,
            machine.bus.now.get(),
        )
        .unwrap();
        assert!(pending.any_bank_access);
        assert!(pending.bank_access.is_none());
        assert!(pending.reservation().is_none());
        machine.sound_schedule.pending = Some(pending);
        machine.bus.sound.request(true);
        machine.bus.ram[0x100..0x102].copy_from_slice(&0xa55a_u16.to_be_bytes());
        let clock = machine.bus.cpu_clock;
        assert_eq!(
            machine
                .bus
                .read_cycle(
                    access(0xff0100, AccessKind::Prefetch, ByteLanes::Both),
                    clock
                )
                .result,
            Ok(0xa55a)
        );
        assert_eq!(machine.bus.prefetch, Prefetch::Full(0xa55a));
        let mut writer = StateWriter::new();
        machine
            .sound_schedule
            .write_state(&machine.sound_cpu, &mut writer)
            .unwrap();
        let encoded = writer.into_bytes();
        let mut restored = machine.clone();
        let mut reader = StateReader::new(&encoded);
        restored
            .sound_schedule
            .read_state(&mut restored.sound_cpu, &mut reader)
            .unwrap();
        assert!(reader.is_exhausted());
        assert_eq!(restored, machine);
        let restored_pending = restored.sound_schedule.pending.as_ref().unwrap();
        assert!(restored_pending.any_bank_access);
        assert!(restored_pending.bank_access.is_none());
        assert!(restored_pending.reservation().is_none());
        let result = cycle(&mut machine, access(address, AccessKind::Data, lanes));
        assert_eq!(
            cycle(&mut restored, access(address, AccessKind::Data, lanes)),
            result
        );
        assert_eq!(restored, machine);
        assert_eq!(result.wait_clocks, 0);
        assert_eq!(
            result.result,
            if lanes == ByteLanes::Upper {
                Ok(0x100)
            } else {
                unsupported(address)
            }
        );
        assert_eq!(machine.bus.prefetch, Prefetch::Empty);
        assert!(
            machine
                .sound_schedule
                .pending
                .as_ref()
                .unwrap()
                .any_bank_access
        );
        hash.update(format!("{address:x}{lanes:?}{result:?}").as_bytes());
    }
    emit("busreq-active-bank-v1", hash, 4);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn busreq_active_word_aliases_retain_only_data_word_history() {
    let mut hash = Sha256::new();
    let mut cases = 0;
    for phase in [Phase::Crossing, Phase::Pending] {
        for offset in (0..=0xfe).step_by(2) {
            for kind in [
                AccessKind::Data,
                AccessKind::Opcode,
                AccessKind::Extension,
                AccessKind::Prefetch,
            ] {
                let mut machine = fixture(Region::NtscUs, phase, 1);
                let address = SOUND_BUS_REQUEST + offset;
                let result = cycle(&mut machine, access(address, kind, ByteLanes::Both));
                assert_eq!(result.wait_clocks, 0);
                assert_eq!(
                    result.result,
                    if kind == AccessKind::Data {
                        Ok(value(phase))
                    } else {
                        unsupported(address)
                    }
                );
                assert_eq!(machine.bus.prefetch, Prefetch::Empty);
                assert_eq!(machine.bus.sound.granted, phase == Phase::Crossing);
                hash.update(format!("{phase:?}{offset}{kind:?}{result:?}").as_bytes());
                cases += 1;
            }
        }
    }
    emit("busreq-active-alias-v1", hash, cases);
}
