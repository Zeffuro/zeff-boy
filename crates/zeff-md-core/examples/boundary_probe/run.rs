use sha2::{Digest, Sha256};
use zeff_md_core::{Emulator, MachineFault};

#[derive(Debug, PartialEq, Eq)]
pub enum Stop {
    Reached,
    Limit,
    Fault(MachineFault),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub stop: Stop,
    pub attempted: u64,
    pub completed: u64,
    pub drained_samples: u64,
    pub audio_sha256: [u8; 32],
}

pub fn run(
    machine: &mut Emulator,
    max_boundaries: u64,
    reached: impl Fn(&Emulator) -> bool,
) -> Report {
    let mut report = Report {
        stop: Stop::Limit,
        attempted: 0,
        completed: 0,
        drained_samples: 0,
        audio_sha256: [0; 32],
    };
    let mut hash = Sha256::new();
    let mut audio = Vec::new();
    loop {
        if reached(machine) {
            report.stop = Stop::Reached;
            break;
        }
        if report.attempted == max_boundaries {
            break;
        }
        report.attempted += 1;
        let step = machine.step_instruction();
        audio.clear();
        machine.drain_audio_samples_into(&mut audio);
        report.drained_samples += audio.len() as u64;
        for sample in &audio {
            hash.update(sample.to_bits().to_le_bytes());
        }
        match step {
            Ok(_) => report.completed += 1,
            Err(fault) => {
                report.stop = Stop::Fault(fault);
                break;
            }
        }
    }
    report.audio_sha256 = hash.finalize().into();
    report
}
