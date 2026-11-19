use super::*;
use crate::hardware::sound::observer::{Metric, Session, Verification};

fn execute(case: u8) -> (String, Vec<u32>) {
    let (program, cut, epoch): (&[u8], _, _) = match case {
        0 => (&[0x32, 0, 0x80, 0x76], 194, false),
        1 => (&[0; 32], 61, true),
        2 => (&[0x3a, 0, 0x7f], 40, false),
        _ => unreachable!(),
    };
    let mut rig = Rig::new(program, cut, epoch, true, 0x1ff, false);
    let mut cpu = rig.main();
    let mut bus = Recorded {
        entry: ResetEntry::new(rig.target(true)).unwrap(),
        events: vec![],
    };
    let result = reset(&mut cpu, &mut bus);
    assert_eq!(result.is_err(), case == 2);
    let events = bus.events.clone();
    drop(bus);
    if result.is_ok() {
        cpu.finish_external_reset_boundary().unwrap();
    }
    rig.close_epoch();
    rig.finish_batch(true);
    if result.is_ok() {
        rig.advance(true, 1_000);
    }
    let _verification = Verification::begin();
    let signature = format!("{result:?}|{events:?}|{cpu:?}|{rig:?}");
    let pcm = rig.pcm();
    (signature, pcm)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_candidate_reset_success_epoch_failure_lifetimes() {
    let mut hash = 0xcbf29ce484222325;
    for case in 0..3 {
        let expected = execute(case);
        let session = Session::begin();
        let actual = execute(case);
        assert_eq!(actual, expected);
        let report = session.finish();
        assert!(report.closed(), "{report:?}");
        assert!(!report.full_coverage);
        assert_eq!(report.scope, "DirectedOwnerThreadOnly");
        assert_eq!(
            report.count(Metric::OwnersCreated),
            report.count(Metric::OwnersClosed)
        );
        assert_eq!(
            report.count(Metric::ImagesCreated),
            report.count(Metric::ImagesClosed)
        );
        assert!(report.count(Metric::Attempts) > 0);
        assert_eq!(
            report.count(Metric::Attempts),
            report.count(Metric::Successes) + report.count(Metric::Failures)
        );
        assert_eq!(report.count(Metric::Failures) > 0, case == 2);
        record(&mut hash, (case, report));
    }
    emit("md-observer-reset-lifecycle-v1", hash, 3);
}
