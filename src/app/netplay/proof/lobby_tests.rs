use super::*;
use crate::netplay::test_lobby::TestLobby;
use zeff_netplay::rollback::InputDelay;

#[test]
fn headless_lobby_route_checks_output_pause_chat_restore_and_hides_credentials() {
    check_pair(false);
}

#[test]
fn nominal_cadence_over_rtc_checks_every_confirmed_frame() {
    check_pair(true);
}

fn check_pair(cadence: bool) {
    let lobby = TestLobby::start();
    let directory = crate::test_support::test_directory("headless-lobby-proof").unwrap();
    let roots = [directory.path().join("host"), directory.path().join("join")];
    let url = lobby.url.clone();
    let host_root = roots[0].clone();
    let host = std::thread::spawn(move || peer(host_root, 0, url, None, cadence));
    let invited = wait_file(&roots[0].join("invitation.txt"), &[&host]);
    let invitation = std::fs::read_to_string(roots[0].join("invitation.txt"));
    if invited.is_err() || invitation.is_err() {
        let _ = host.join();
        panic!("host failed to create a private invitation file");
    }
    let invitation = invitation.unwrap();
    let secret = invitation.split('/').nth(2).unwrap().to_owned();
    let join_root = roots[1].clone();
    let url = lobby.url.clone();
    let join = std::thread::spawn(move || peer(join_root, 1, url, Some(invitation), cadence));
    let controlled = controller(&roots, &[&host, &join], cadence);
    let one = host.join().unwrap();
    let two = join.join().unwrap();
    controlled.unwrap();
    for result in [one, two] {
        let report = result.unwrap();
        assert!(report["admitted"].as_bool().unwrap());
        assert!(report["exact_restore"].as_bool().unwrap());
        assert!(report["save_protection_after_shutdown"].as_bool().unwrap());
        assert_eq!(report["reference_checked_frames"], report["frames"]);
        assert_eq!(report["chat_messages"], if cadence { 2 } else { 4 });
        if cadence {
            assert_eq!(report["frames"], 120);
            assert_eq!(report["pause_rounds"], 0);
            assert_eq!(
                report["cadence"]["verification"]["reference_checked_frames"],
                120
            );
            assert_eq!(report["cadence"]["verification"]["submitted_samples"], 120);
            assert_eq!(report["cadence"]["measured_intervals"], 60);
        }
        assert_eq!(report["scope"], "direct-dtls-sctp");
        assert_eq!(report["transport"], "webrtc-data-channel");
        assert!(report["local_endpoint"].is_null());
        assert!(!report.to_string().contains(&secret));
        assert!(!report.to_string().contains("local-private-key-marker"));
    }
    assert_eq!(lobby.lobby.room_count(), 0);
}

fn peer(
    root: std::path::PathBuf,
    role: usize,
    url: String,
    invitation: Option<String>,
    cadence: bool,
) -> Result<Value> {
    std::fs::create_dir(&root)?;
    let options = Options {
        root,
        role,
        frames: if cadence { 120 } else { 24 },
        input_delay: InputDelay::new(2)?,
        address: "127.0.0.1:0".parse()?,
        route: route::Route::Lobby(crate::netplay::connect::lobby::Options {
            url,
            access_token: "local-private-key-marker".into(),
            input_delay: InputDelay::new(2)?,
        }),
        invitation,
        reject_build: false,
        jitter_ms: 0,
        paced: false,
        cadence,
        fault: None,
        fault_role: 0,
        cartridge: None,
    };
    let path = options.copy_media()?;
    execute(&options, &path)
}

fn controller(
    roots: &[std::path::PathBuf; 2],
    peers: &[&std::thread::JoinHandle<Result<Value>>],
    cadence: bool,
) -> Result<()> {
    for stage in 0..if cadence { 0 } else { 4 } {
        let name = format!("pause-stage-{stage}");
        for root in roots {
            wait_file(&root.join(format!("{name}.json")), peers)?;
        }
        for root in roots {
            write_new(&root.join(format!("{name}.continue")), b"ready")?;
        }
    }
    for root in roots {
        wait_file(&root.join("ready.json"), peers)?;
    }
    for root in roots {
        write_new(&root.join("finish"), b"ready")?;
    }
    Ok(())
}

fn wait_file(path: &Path, peers: &[&std::thread::JoinHandle<Result<Value>>]) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.exists() {
        ensure!(
            !peers.iter().any(|peer| peer.is_finished()),
            "proof peer ended before barrier"
        );
        ensure!(
            Instant::now() < deadline,
            "proof controller barrier deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}
