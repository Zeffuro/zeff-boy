use super::*;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_actual_cpu_push_465() {
    let mut controls = Vec::new();
    {
        let mut cartridge = cartridge();
        let mut memory = memory();
        let mut cpu = Cpu::new();
        let mut schedule = SoundSchedule::default();
        let mut audio = Audio::default();
        let mut cursor = 0;
        for deadline in FENCES {
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cartridge,
                &mut [],
                SoundInterval::new(cursor..deadline, &mut audio),
            );
            controls.push(signature(&cpu, &memory, &schedule, &audio));
            cursor = deadline;
        }
    }
    let mut cartridge = cartridge();
    let mut memory = memory();
    let mut cpu = Cpu::new();
    let mut schedule = SoundSchedule::default();
    let mut audio = Audio::default();
    let session = Session::begin();
    memory.ram.enroll_observer();
    let mut cursor = 0;
    for (index, deadline) in FENCES.into_iter().enumerate() {
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(cursor..deadline, &mut audio),
        );
        assert!(schedule.fault.is_none());
        assert!(schedule.pending.is_none());
        let report = session.report();
        assert_eq!(report.flags, 0);
        assert_eq!(report.count(Metric::Attempts), index as u64 + 1);
        assert_eq!(report.count(Metric::Successes), index as u64 + 1);
        assert_eq!(report.count(Metric::ImagesCreated), index as u64 + 1);
        assert_eq!(report.count(Metric::ImagesClosed), index as u64 + 1);
        assert_eq!(report.count(Metric::Publications), index as u64 + 1);
        assert_eq!(report.count(Metric::Failures), 0);
        {
            let _verification = Verification::begin();
            assert_eq!(signature(&cpu, &memory, &schedule, &audio), controls[index]);
            assert_eq!(memory.ram[0x101], if deadline >= 420 { 0x12 } else { 0 });
            assert_eq!(memory.ram[0x100], if deadline >= 465 { 0x34 } else { 0 });
        }
        cursor = deadline;
    }
    let at465 = session.report();
    assert_eq!(at465.count(Metric::ZeroWrites), 8);
    assert_eq!(at465.count(Metric::OneWrite), 2);
    assert_eq!(at465.count(Metric::TwoWrites), 0);
    assert_eq!(at465.count(Metric::ManyWrites), 0);
    assert_eq!(at465.count(Metric::RamWrites), 2);
    assert_eq!(at465.count(Metric::DeepCopies), 0);
    assert_eq!(at465.count(Metric::ImageClones), 0);
    assert_eq!(at465.count(Metric::SharedDetaches), 2);
    assert_eq!(at465.count(Metric::CopyBytes), 16384);
    assert_eq!(at465.live_owners, 1);
    drop(schedule);
    drop(memory);
    let final_report = session.finish();
    assert!(final_report.closed(), "{final_report:?}");
    assert!(!final_report.full_coverage);
    assert_eq!(
        final_report.count(Metric::OwnersCreated),
        final_report.count(Metric::OwnersClosed)
    );
    assert_eq!(
        final_report.counters,
        [
            21, 21, 3, 20, 0, 2, 2, 0, 16384, 10, 10, 0, 10, 10, 0, 10, 7, 2, 0, 0, 8, 2, 0, 0
        ]
    );
    super::super::super::observer_contract_tests::emit(format!(
        "OBSERVER_PUSH_465={final_report:?}"
    ));
}
