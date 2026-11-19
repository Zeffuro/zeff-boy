use super::super::tests::{guest, lease};
use super::super::{MdAllocationKind as K, MdRetainedAllocations};
use super::*;
use crate::{Region, TimingModel};
use zeff_emu_common::time::MasterTicks;

fn hashes(core: &Emulator) -> MdRollbackHashes {
    MdRollbackConfig::capture(core)
        .unwrap()
        .boundary_hashes(core)
        .unwrap()
}
fn observe(
    owner: &MdRollbackSession,
    a: &MdRollbackSnapshot,
    b: &MdRollbackSnapshot,
    candidate: &Emulator,
    transfer: &Emulator,
) -> super::super::MdRetainedPayload {
    let mut into = MdRetainedAllocations::new();
    owner.visit_retained_allocations(&mut into).unwrap();
    a.visit_retained_allocations(&mut into).unwrap();
    b.visit_retained_allocations(&mut into).unwrap();
    into.visit_emulator(candidate).unwrap();
    into.visit_emulator(transfer).unwrap();
    into.finish().unwrap()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn supplements_bind_runtime_omissions_without_normalizing_the_machine() {
    let mut core = guest(Region::NtscUs, TimingModel::Standard, true, false);
    core.step_field().unwrap();
    let before = core.clone();
    let base = hashes(&core);
    let config = MdRollbackConfig::capture(&core).unwrap();
    assert!(core == before);
    let (epoch, hint) = core.bus.vdp.rollback_supplements();
    let hint = hint.map(|(region, now, end)| {
        (
            match region {
                0 => Region::NtscUs,
                1 => Region::NtscJapan,
                2 => Region::Pal,
                _ => unreachable!(),
            },
            MasterTicks::new(now),
            MasterTicks::new(end),
        )
    });
    let mut changed = core.clone();
    changed
        .bus
        .vdp
        .rollback_test_supplements(epoch.checked_add(1).unwrap(), hint);
    assert_ne!(hashes(&changed).logical(), base.logical());
    let mut changed = core.clone();
    changed.bus.vdp.rollback_test_supplements(
        epoch,
        Some((
            core.region(),
            core.bus.now,
            MasterTicks::new(core.bus.now.get() + 1),
        )),
    );
    assert_eq!(hashes(&changed).logical(), base.logical());
    assert_ne!(hashes(&changed).local(), base.local());

    let mut changed = core.clone();
    changed
        .decode_frontend_state(&core.encode_frontend_state().unwrap())
        .unwrap();
    assert_eq!(
        changed.encode_frontend_state().unwrap(),
        core.encode_frontend_state().unwrap()
    );
    assert_ne!(hashes(&changed).logical(), base.logical());
    let mut trace = core.clone();
    trace.reset_and_begin_audio_trace(64).unwrap();
    trace.finish_audio_trace().unwrap();
    let mut changed = core.clone();
    changed.bus.audio.trace = trace.bus.audio.trace;
    assert_eq!(hashes(&changed).logical(), base.logical());
    assert_ne!(hashes(&changed).local(), base.local());

    let mut changed = core.clone();
    changed.bus.audio.rollback_test_phase();
    assert_ne!(hashes(&changed).logical(), base.logical());
    let mut changed = core.clone();
    changed.bus.audio.rollback_test_live_pcm();
    assert_ne!(hashes(&changed).logical(), base.logical());
    let mut changed = core.clone();
    changed.bus.audio.rollback_test_storage();
    assert_eq!(hashes(&changed).logical(), base.logical());
    assert_eq!(hashes(&changed).local(), base.local());

    let mut epoch = core.clone();
    epoch.sound_schedule.epoch = Some(Default::default());
    let epoch_before = epoch.clone();
    assert!(config.boundary_hashes(&epoch).is_err());
    assert!(epoch == epoch_before);
    let mut mailbox = core.clone();
    mailbox.cpu.set_control_flow_recording(true);
    let context = mailbox.begin_debug_call(0x200).unwrap();
    assert!(!mailbox.debug_call_returned(&context));
    let pending = mailbox.cpu.clone().drain_control_flow_events();
    assert!(pending.events().next().is_some());
    assert!(config.boundary_hashes(&mailbox).is_err());
    let retained = mailbox.cpu.clone().drain_control_flow_events();
    assert_eq!(pending, retained);
    assert!(mailbox.cpu.control_flow_recording());
    assert!(core == before);
    assert_eq!(hashes(&core), base);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn retained_payload_tracks_shared_media_deep_ram_cow_and_real_capacities() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let core = guest(Region::NtscUs, timing, true, false);
        let bytes = core.rom_bytes().len() as u64;
        let owner = lease(core);
        let a = owner.capture().unwrap();
        let b = owner.capture().unwrap();
        let mut candidate = owner.core().clone();
        let transfer = owner.core().clone();
        let before = observe(&owner, &a, &b, &candidate, &transfer);
        assert_eq!(before.count(K::CoreLayout), 6);
        assert_eq!(before.count(K::Rom), 1);
        assert_eq!(before.bytes(K::Rom), bytes);
        assert_eq!(before.count(K::OwnerToken), 1);
        assert_eq!(before.count(K::AudioSamples), 6);
        assert_eq!(
            before.bytes(K::SoundRam),
            before.count(K::SoundRam) * crate::constants::SOUND_RAM_SIZE as u64
        );
        if timing == TimingModel::Standard {
            assert_eq!(before.count(K::SoundSemantic), 0);
            assert_eq!(before.count(K::SoundRam), 6);
        } else {
            assert_eq!(before.count(K::SoundSemantic), 1);
            let original = hashes(&candidate);
            let (before_capacity, capacity) = candidate
                .bus
                .owned
                .as_mut()
                .unwrap()
                .rollback_test_spare_audio(17);
            assert!(capacity > before_capacity);
            let spare = observe(&owner, &a, &b, &candidate, &transfer);
            assert_eq!(
                spare.bytes(K::AudioPending) - before.bytes(K::AudioPending),
                ((capacity - before_capacity)
                    * size_of::<crate::hardware::vdp::owner::sound_cpu::AudioEffect>())
                    as u64
            );
            assert_eq!(hashes(&candidate), original);
            candidate
                .bus
                .owned
                .as_mut()
                .unwrap()
                .rollback_test_sound_cow(0x5a);
            let cow = observe(&owner, &a, &b, &candidate, &transfer);
            assert_eq!(cow.count(K::SoundSemantic), 2);
            assert_eq!(
                cow.bytes(K::SoundRam),
                spare.bytes(K::SoundRam) + crate::constants::SOUND_RAM_SIZE as u64
            );
            candidate.bus.video.rollback_test_last_frame();
            let frame = observe(&owner, &a, &b, &candidate, &transfer);
            assert_eq!(
                frame.bytes(K::SurfaceLastPixels),
                320 * 240 * size_of::<u32>() as u64
            );
        }
        let mut into = MdRetainedAllocations::new();
        owner.visit_retained_allocations(&mut into).unwrap();
        let once = into.finish().unwrap();
        owner.visit_retained_allocations(&mut into).unwrap();
        assert_eq!(into.finish().unwrap(), once);
    }
    let first = guest(Region::NtscUs, TimingModel::Standard, true, false);
    let second = guest(Region::NtscUs, TimingModel::Standard, true, false);
    assert_eq!(first.rom_bytes(), second.rom_bytes());
    let mut distinct = MdRetainedAllocations::new();
    distinct.visit_emulator(&first).unwrap();
    distinct.visit_emulator(&second).unwrap();
    assert_eq!(distinct.finish().unwrap().count(K::Rom), 2);
    let mut overflow = MdRetainedAllocations::new();
    if overflow.payload(K::WorkRam, usize::MAX, usize::MAX).is_ok() {
        assert!(
            overflow
                .payload(K::WorkRam, usize::MAX, usize::MAX)
                .is_err()
        );
    }
    assert!(overflow.finish().is_err());
    let mut trace = guest(Region::NtscUs, TimingModel::Standard, true, false);
    trace.reset_and_begin_audio_trace(64).unwrap();
    let mut into = MdRetainedAllocations::new();
    into.visit_emulator(&trace).unwrap();
    let trace_cost = into.finish().unwrap();
    assert!(trace.bus.audio.trace.retained_event_capacity() >= 64);
    assert_eq!(
        trace_cost.bytes(K::AudioTraceEvents),
        trace.bus.audio.trace.retained_event_capacity() as u64
            * size_of::<
                zeff_emu_common::audio_trace::AudioTraceEvent<
                    zeff_emu_common::audio_trace::MdTraceWrite,
                >,
            >() as u64
    );
    assert!(MdRollbackConfig::capture(&trace).is_err());
}
