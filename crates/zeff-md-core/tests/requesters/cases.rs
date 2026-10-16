#[path = "../../src/hardware/bus/ownership.rs"]
mod ownership;

use ownership::{Requester, Reservation};
use std::ops::Range;

const THIRD: Requester = Requester::VDP;
const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];
const OWNERS: [Requester; 3] = [Requester::MAIN, Requester::SOUND, THIRD];

fn slot(requester: Requester, ticks: Range<u64>) -> Reservation {
    Reservation { requester, ticks }
}

pub struct WaitCase {
    candidate: Reservation,
    occupied: [Reservation; 3],
    count: usize,
    quantum: u64,
    pub expected: Option<u64>,
}

impl WaitCase {
    pub fn run(&self) -> Option<u64> {
        self.candidate
            .wait_after(&self.occupied[..self.count], self.quantum)
    }
}

fn row(
    ticks: Range<u64>,
    occupied: [Range<u64>; 3],
    count: usize,
    quantum: u64,
    expected: Option<u64>,
) -> WaitCase {
    WaitCase {
        candidate: slot(Requester::MAIN, ticks),
        occupied: occupied.map(|ticks| slot(Requester::SOUND, ticks)),
        count,
        quantum,
        expected,
    }
}

pub fn wait_ledger() -> [WaitCase; 24] {
    let max = u64::MAX;
    let mut alias = row(10..14, [0..100, 0..0, 0..0], 1, 3, Some(0));
    alias.occupied[0].requester = Requester(0);
    let mut third = row(10..14, [11..15, 0..0, 0..0], 1, 3, Some(2));
    third.occupied[0].requester = THIRD;
    let mut third_candidate = row(10..14, [11..15, 0..0, 0..0], 1, 3, Some(2));
    third_candidate.candidate.requester = THIRD;
    let mut chain = row(10..14, [11..15, 17..22, 24..28], 3, 3, Some(6));
    chain.occupied[1].requester = THIRD;
    [
        row(10..14, [0..0, 0..0, 0..0], 0, 3, Some(0)),
        row(10..14, [5..10, 14..20, 0..0], 2, 3, Some(0)),
        row(10..14, [12..12, 0..0, 0..0], 1, 3, Some(0)),
        row(10..10, [0..20, 0..0, 0..0], 1, 3, Some(0)),
        row(10..14, [11..15, 0..0, 0..0], 1, 3, Some(2)),
        row(10..14, [10..14, 0..0, 0..0], 1, 3, Some(2)),
        row(10..14, [0..11, 0..0, 0..0], 1, 3, Some(1)),
        row(10..14, [11..15, 17..22, 0..0], 2, 3, Some(4)),
        row(10..14, [11..15, 17..22, 24..28], 3, 3, Some(6)),
        row(10..14, [11..18, 12..22, 0..0], 2, 3, Some(4)),
        row(10..14, [20..30, 0..0, 0..0], 1, 3, Some(0)),
        row(Range { start: 4, end: 3 }, [0..0, 0..0, 0..0], 0, 1, None),
        row(0..1, [Range { start: 4, end: 3 }, 0..0, 0..0], 1, 1, None),
        row(0..0, [Range { start: 4, end: 3 }, 0..0, 0..0], 1, 1, None),
        row(0..1, [0..0, 0..0, 0..0], 0, 0, None),
        row(
            max - 4..max - 2,
            [max - 4..max - 2, 0..0, 0..0],
            1,
            1,
            Some(2),
        ),
        row(max - 2..max, [max - 2..max, 0..0, 0..0], 1, 1, None),
        row(0..1, [0..max, 0..0, 0..0], 1, 2, None),
        row(max - 3..max - 1, [max - 3..max - 1, 0..0, 0..0], 1, 3, None),
        row(max..max, [0..max, 0..0, 0..0], 1, max, Some(0)),
        alias,
        third,
        third_candidate,
        chain,
    ]
}

pub fn literal_wait_ledger() {
    for case in wait_ledger() {
        assert_eq!(case.run(), case.expected);
    }
}

pub fn three_requesters_all_arrival_orders() {
    for order in ORDERS {
        let mut granted = OWNERS.map(|owner| slot(owner, 0..0));
        for (arrival, index) in order.into_iter().enumerate() {
            let candidate = slot(OWNERS[index], 0..2);
            let delay = candidate.wait_after(&granted[..arrival], 1).unwrap();
            assert_eq!(delay, [0, 2, 4][arrival]);
            granted[arrival] = slot(candidate.requester, delay..delay + 2);
        }
        assert_eq!(
            granted.clone().map(|slot| slot.requester),
            order.map(|index| OWNERS[index])
        );
        assert_eq!(granted.map(|slot| slot.ticks), [0..2, 2..4, 4..6]);
    }
}

pub fn chained_conflicts_ignore_reservation_order() {
    let occupied = [
        slot(Requester::SOUND, 11..15),
        slot(THIRD, 17..22),
        slot(Requester::SOUND, 24..28),
    ];
    for order in ORDERS {
        let reordered = order.map(|index| occupied[index].clone());
        assert_eq!(
            slot(Requester::MAIN, 10..14).wait_after(&reordered, 3),
            Some(6)
        );
    }
}

pub fn equality_only_identity_and_aliases() {
    assert_ne!(Requester::MAIN, Requester::SOUND);
    assert_ne!(Requester::MAIN, THIRD);
    assert_ne!(Requester::SOUND, THIRD);
    for owner in OWNERS {
        let alias = Requester(owner.0);
        let candidate = slot(owner, 10..14);
        assert_eq!(candidate.wait_after(&[slot(alias, 0..100)], 3), Some(0));
        assert_eq!(
            candidate.wait_after(&[slot(alias, Range { start: 8, end: 7 })], 3),
            None
        );
        for other in OWNERS.into_iter().filter(|other| *other != owner) {
            assert_eq!(candidate.wait_after(&[slot(other, 11..15)], 3), Some(2));
        }
    }
    for (first, second) in [(0, 255), (255, 0)] {
        assert_eq!(
            slot(Requester(first), 10..14).wait_after(&[slot(Requester(second), 11..15)], 3),
            Some(2)
        );
    }
}

pub fn exact_ties_preserve_grant_sequence() {
    let pending = [
        slot(THIRD, 20..24),
        slot(Requester::MAIN, 20..24),
        slot(Requester::SOUND, 20..24),
    ];
    let before = pending.clone();
    let mut granted = OWNERS.map(|owner| slot(owner, 0..0));
    let mut completions = [0; 3];
    for (index, candidate) in pending.iter().enumerate() {
        let wait = candidate.wait_after(&granted[..index], 2).unwrap();
        assert_eq!(wait, [0, 2, 4][index]);
        let offset = wait * 2;
        granted[index] = slot(
            candidate.requester,
            candidate.ticks.start + offset..candidate.ticks.end + offset,
        );
        completions[index] = granted[index].ticks.end;
    }
    assert_eq!(completions, [24, 28, 32]);
    assert_eq!(
        granted.map(|slot| slot.requester),
        [THIRD, Requester::MAIN, Requester::SOUND]
    );
    assert_eq!(pending, before);
}

pub fn unchanged_inputs_and_clones() {
    for case in wait_ledger() {
        let candidate = case.candidate.clone();
        let occupied = case.occupied.clone();
        let original = (candidate.clone(), occupied.clone());
        assert_eq!(
            candidate.wait_after(&occupied[..case.count], case.quantum),
            case.expected
        );
        assert_eq!((candidate, occupied), original);
        assert_eq!((case.candidate, case.occupied), original);
    }
}
