use super::*;
use crate::emu_backend::EmuBackend;
use crate::emu_thread::EmuThread;
use crate::netplay::md_fixture as fixture;
use crate::netplay::session::md_start_failure::Stage;
use zeff_md_core::{ControllerKind, Region, TimingModel};
use zeff_netplay::lockstep::Player;

pub(super) fn worker_and_start(
    stage: Stage,
) -> (
    Option<crate::test_support::TestDirectory>,
    EmuThread,
    crate::netplay::Start,
    crate::netplay::Transport,
) {
    let config = fixture::config(
        Region::NtscUs,
        TimingModel::Standard,
        ControllerKind::ThreeButton,
    );
    #[cfg(not(target_arch = "wasm32"))]
    let (directory, mut backend) = {
        let (directory, backend) = fixture::loaded(config);
        (Some(directory), backend)
    };
    #[cfg(target_arch = "wasm32")]
    let (directory, mut backend) = (None, fixture::loaded(config));
    let EmuBackend::Md(md) = &mut backend else {
        unreachable!()
    };
    md.arm_start_failure_for_test(stage).unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    let (stream, peer) = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (peer, _) = listener.accept().unwrap();
        (stream.into(), peer.into())
    };
    #[cfg(target_arch = "wasm32")]
    let (stream, peer) = {
        let (stream, peer, _) = crate::netplay::network::paired::pair();
        (
            crate::netplay::Transport::Paired(stream),
            crate::netplay::Transport::Paired(peer),
        )
    };
    let start = crate::netplay::Start {
        stream,
        player: Player::One,
        build: crate::netplay::connect::executable_build().unwrap(),
        secret: [5; 32],
        scope: ConnectionScope::Loopback,
        allow_different_versions: false,
        verify_every_frame: true,
        input_delay: zeff_netplay::rollback::InputDelay::new(0).unwrap(),
    };
    (directory, EmuThread::spawn(backend, false), start, peer)
}

pub(super) fn next_response(worker: &EmuThread) -> Response {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        #[cfg(not(target_arch = "wasm32"))]
        let response = match worker.poll_response() {
            crate::emu_thread::EmuResponsePoll::Response(response) => Some(*response),
            crate::emu_thread::EmuResponsePoll::Empty => None,
            crate::emu_thread::EmuResponsePoll::Disconnected => panic!("worker disconnected"),
        };
        #[cfg(target_arch = "wasm32")]
        let response = worker.try_recv_response();
        if let Some(response) = response {
            let EmuResponse::Netplay(response) = response else {
                panic!("unexpected worker response")
            };
            return response;
        }
        assert!(Instant::now() < deadline);
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::yield_now();
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_failed_start_worker_response_keeps_shared_app_frontend_fenced_after_failed_abort() {
    for phase in [Phase::Admission, Phase::Stopping] {
        for stage in [Stage::FirstCapture, Stage::AbortVerification] {
            let (_directory, mut worker, start, _peer) = worker_and_start(stage);
            worker.send(EmuCommand::StartNetplay(Box::new(start)));
            let response = next_response(&worker);
            let poisoned = stage == Stage::AbortVerification;
            assert_eq!(
                matches!(
                    &response,
                    Response::Stopped {
                        restored: false,
                        ..
                    }
                ),
                poisoned
            );
            assert_eq!(matches!(&response, Response::Rejected(_)), !poisoned);
            let mut frontend = Frontend {
                phase: if phase == Phase::Admission {
                    Phase::Admission
                } else {
                    Phase::Stopping
                },
                ..Default::default()
            };
            let restored = frontend.terminal_restore(&response).unwrap();
            assert_eq!(restored, !poisoned);
            frontend.finish(restored);
            assert_eq!(frontend.fenced(), poisoned);
            assert!(if poisoned {
                frontend.phase == Phase::Poisoned
            } else {
                frontend.phase == Phase::Idle
            });
            assert!(!frontend.running());
            assert!(!frontend.in_flight);
            if poisoned {
                worker.send(EmuCommand::Reset);
                let refusal = next_response(&worker);
                assert!(matches!(
                    &refusal,
                    Response::Stopped {
                        restored: false,
                        ..
                    }
                ));
                let restored = frontend.terminal_restore(&refusal).unwrap();
                assert!(!restored);
                frontend.finish(restored);
                assert!(frontend.fenced());
            }
            worker.shutdown();
        }
    }
}
