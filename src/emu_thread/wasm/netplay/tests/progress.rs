use super::*;

#[derive(Default)]
pub(super) struct Progress {
    confirmed: [usize; 2],
    last: [Duration; 2],
}

impl Progress {
    pub(super) fn observe(&mut self, elapsed: Duration, confirmed: [usize; 2]) -> bool {
        if elapsed >= Duration::from_secs(30)
            || self
                .ages(elapsed)
                .into_iter()
                .zip(self.confirmed)
                .any(|(age, count)| count < 72 && age >= Duration::from_secs(3))
        {
            return false;
        }
        for (i, count) in confirmed.into_iter().enumerate() {
            if count < self.confirmed[i] {
                return false;
            }
            if count > self.confirmed[i] {
                self.last[i] = elapsed;
            }
            self.confirmed[i] = count;
        }
        true
    }

    pub(super) fn ages(&self, elapsed: Duration) -> [Duration; 2] {
        self.last.map(|last| elapsed.saturating_sub(last))
    }
}

#[wasm_bindgen_test]
fn browser_netplay_progress_requires_both_confirmations_and_a_total_bound() {
    let mut progress = Progress::default();
    for second in 1..=29 {
        assert!(progress.observe(Duration::from_secs(second), [second as usize; 2]));
    }
    assert!(!progress.observe(Duration::from_secs(30), [30; 2]));
    assert_eq!(
        progress.ages(Duration::from_secs(30)),
        [Duration::from_secs(1); 2]
    );

    let mut progress = Progress::default();
    assert!(progress.observe(Duration::from_secs(1), [1, 1]));
    assert!(progress.observe(Duration::from_secs(2), [2, 1]));
    assert!(progress.observe(Duration::from_secs(3), [3, 1]));
    assert!(!progress.observe(Duration::from_secs(4), [4, 1]));
    assert_eq!(
        progress.ages(Duration::from_secs(4)),
        [Duration::from_secs(1), Duration::from_secs(3)]
    );

    let mut progress = Progress::default();
    assert!(progress.observe(Duration::from_secs(2), [0, 0]));
    assert!(!progress.observe(Duration::from_secs(3), [0, 0]));

    let mut progress = Progress::default();
    assert!(!progress.observe(Duration::from_secs(3), [1, 1]));

    let mut progress = Progress::default();
    assert!(progress.observe(Duration::from_secs(1), [72, 1]));
    for second in 2..=6 {
        assert!(progress.observe(Duration::from_secs(second), [72, second as usize]));
    }
    assert!(!progress.observe(Duration::from_secs(7), [72, 5]));
}
