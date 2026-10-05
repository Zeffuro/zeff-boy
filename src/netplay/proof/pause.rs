use super::*;
use std::sync::Arc;

pub(super) fn requests(workers: [&EmuThread; 2], paused: [bool; 2]) {
    for (worker, paused) in workers.into_iter().zip(paused) {
        worker.send(EmuCommand::SetNetplayPaused(paused));
    }
}

pub(super) fn hold(
    workers: [&EmuThread; 2],
    frame: u64,
    reference: &mut EmuBackend,
    config: [u8; 32],
) -> Result<u64> {
    requests(workers, [true, false]);
    let lead = zeff_netplay::rollback::PREDICTION_WINDOW + 2 * zeff_netplay::lockstep::INPUT_DELAY;
    for next in frame..frame + lead {
        for (index, worker) in workers.into_iter().enumerate() {
            worker.send(EmuCommand::StepNetplay(sample(
                next,
                if index == 0 { Player::One } else { Player::Two },
            )));
        }
        let ports = [sample(next - 2, Player::One), sample(next - 2, Player::Two)];
        let (expected, audio) = reference_frame(reference, ports, config)?;
        for worker in workers {
            let Response::Frame {
                checkpoint,
                ports: used,
                audio: pcm,
            } = stepped_frame(worker)?
            else {
                bail!("pause approach lost frame")
            };
            ensure!(
                checkpoint == expected && used == ports,
                "pause approach diverged"
            );
            ensure!(
                pcm.iter()
                    .map(|v| v.to_bits())
                    .eq(audio.iter().map(|v| v.to_bits())),
                "pause approach PCM differs"
            );
        }
    }
    let before = workers.map(|worker| worker.shared_framebuffer().load_full().unwrap());
    let mut rounds = 0;
    for flags in [[true, false], [true, true], [false, true]] {
        requests(workers, flags);
        for _ in 0..3 {
            for worker in workers {
                worker.send(EmuCommand::StepNetplay(255));
            }
            for (index, worker) in workers.into_iter().enumerate() {
                loop {
                    match next_any(worker)? {
                        Response::Presented {
                            frame: actual,
                            step_complete: true,
                            changed: false,
                            ..
                        } => {
                            ensure!(actual == frame + lead, "pause advanced");
                            break;
                        }
                        Response::Paused { frame: actual, .. } => {
                            ensure!(actual == frame + lead, "pause boundary differs")
                        }
                        Response::NetworkStats(_) => {}
                        _ => bail!("paused worker executed or committed output"),
                    }
                }
                ensure!(
                    Arc::ptr_eq(
                        &before[index],
                        &worker.shared_framebuffer().load_full().unwrap()
                    ),
                    "paused worker published image"
                );
            }
            rounds += 1;
        }
    }
    Ok(rounds)
}
