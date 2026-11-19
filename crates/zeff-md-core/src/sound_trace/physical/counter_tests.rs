use super::*;

fn stamp(half: u64) -> Stamp {
    Stamp { half, pass: 0 }
}
fn tag(half: u64) -> WorkTag {
    WorkTag {
        stamp: Stamp { half, pass: 1 },
        ordinal: 0,
        lane: 1,
        logical: 0x7f11,
        bank: 0,
        address: 0x1001,
        value: 0x22,
    }
}
fn psg(source: u64, end: u64, sequence: u64) -> PsgWrite {
    PsgWrite {
        binding: crate::hardware::vdp::owner::board::PsgBinding {
            address: 0x7f11,
            bank: 0,
            half: source,
            pass: 1,
        },
        sequence,
        value: 0x22,
        half: end,
        pass: 1,
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_physical_journal_exhausted_interval_blocks_work_and_delayed_psg_fallback() {
    let binding = Binding {
        address: 0x7f11,
        bank: 0,
        shape: Shape::Write,
        external: true,
    };
    let mut journal = Journal::default();
    journal.set_enabled(true);
    journal.last_serial_next();
    let mut first = journal.begin_half(10);
    first.activated(binding);
    first.at(Stamp { half: 10, pass: 1 });
    first.work_tag(tag(10));
    journal.accept_half(first);
    let last = journal
        .events()
        .iter()
        .find_map(|event| match event.fact {
            Fact::Activated(_) => event.occurrence,
            _ => None,
        })
        .unwrap();
    assert_eq!(last.serial, u64::MAX);
    let before_gap = journal.gaps();
    let mut second = journal.begin_half(20);
    second.activated(binding);
    second.at(Stamp { half: 20, pass: 1 });
    second.work_tag(tag(20));
    journal.accept_half(second);
    for (source, end, sequence) in [(10, 21, 1), (20, 22, 2)] {
        let receipt = psg(source, end, sequence);
        receipt.validate().unwrap();
        let mut half = journal.begin_half(end);
        half.at(Stamp { half: end, pass: 1 });
        half.psg(receipt);
        journal.accept_half(half);
    }
    let pulses: Vec<_> = journal
        .events()
        .iter()
        .filter_map(|event| match event.fact {
            Fact::WorkPulse(tag) => Some((tag.stamp, event.occurrence)),
            _ => None,
        })
        .collect();
    assert_eq!(
        pulses,
        [
            (Stamp { half: 10, pass: 1 }, Some(last)),
            (Stamp { half: 20, pass: 1 }, None)
        ]
    );
    let receipts: Vec<_> = journal
        .events()
        .iter()
        .filter_map(|event| match event.fact {
            Fact::PsgAccepted(receipt) => Some((receipt.binding.half, event.occurrence)),
            _ => None,
        })
        .collect();
    assert_eq!(receipts, [(10, Some(last)), (20, None)]);
    assert!(journal.gaps() >= before_gap + 3);
    assert!(journal.events().iter().any(|event| event.stamp == stamp(20)
        && event.occurrence.is_none()
        && matches!(event.fact, Fact::Activated(_))));
}
