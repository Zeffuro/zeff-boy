use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use serde_json::{Value, json};

use super::App;

#[derive(Default)]
pub(super) struct Timings {
    pub(super) rounds: Vec<Duration>,
    pub(super) references: Vec<Duration>,
}

impl Timings {
    pub(super) fn report(&self) -> Value {
        fn summary(samples: &[Duration]) -> Value {
            let mut values: Vec<_> = samples.iter().map(Duration::as_secs_f64).collect();
            values.sort_by(f64::total_cmp);
            if values.is_empty() {
                return json!(null);
            }
            json!({
                "count": values.len(),
                "total_ms": values.iter().sum::<f64>() * 1000.0,
                "median_ms": values[values.len() / 2] * 1000.0,
                "p95_ms": values[(values.len() - 1) * 95 / 100] * 1000.0,
            })
        }
        json!({"round": summary(&self.rounds), "reference_core_and_checkpoint": summary(&self.references)})
    }
}

pub(super) fn paced_round(app: &mut App) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.drain_emu_responses();
        ensure!(
            app.netplay.running(),
            "{}",
            app.debug_windows.netplay.status
        );
        app.pump_netplay();
        if app.netplay.in_flight {
            break;
        }
        ensure!(Instant::now() < deadline, "paced App submission deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    super::wait(app, Duration::from_secs(10), |app| !app.netplay.in_flight)
}
