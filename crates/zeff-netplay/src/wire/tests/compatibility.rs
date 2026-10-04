use super::*;

fn qualified(build: u8, platform: u8, version: &str, consent: bool) -> Identity {
    let mut identity = identity();
    identity.build = [build; 32];
    identity.build_info = BuildInfo {
        version: version.into(),
        contract: [17; 32],
        platform,
        allow_different_versions: consent,
        ..BuildInfo::default()
    };
    identity
}

fn certified(build: u8, peer: u8, platform: u8, version: &str, consent: bool) -> Identity {
    let mut identity = qualified(build, platform, version, consent);
    identity.build_info.certificate = [31; 32];
    identity.build_info.peer_build = [peer; 32];
    identity
}

fn assert_admission(local: Identity, remote: Identity, accepted: bool) {
    let (host, client) = admission_identities(local, remote, Player::Two, [9; 32]);
    if accepted {
        let mut host = host.unwrap();
        let mut client = client.unwrap();
        assert_eq!(host.transcript(), client.transcript());
        host.send(&input(Player::One)).unwrap();
        assert_eq!(client.receive().unwrap(), input(Player::One));
        client.send(&input(Player::Two)).unwrap();
        assert_eq!(host.receive().unwrap(), input(Player::Two));
    } else {
        assert!(host.is_err());
        assert!(client.is_err());
    }
}

#[test]
fn qualified_same_version_different_artifacts_need_no_consent() {
    for remote_platform in [1, 2] {
        assert_admission(
            qualified(1, 1, "1.0", false),
            qualified(2, remote_platform, "1.0", false),
            true,
        );
    }
}

#[test]
fn exact_signed_pair_admits_only_its_mutually_expected_artifacts() {
    let local = certified(1, 2, 1, "1.0", false);
    let remote = certified(2, 1, 2, "1.0", false);
    assert_admission(local.clone(), remote.clone(), true);
    for changed in 0..5 {
        let mut remote = remote.clone();
        match changed {
            0 => remote.build = [3; 32],
            1 => remote.build_info.peer_build = [3; 32],
            2 => remote.build_info.certificate = [32; 32],
            3 => {
                remote.build_info.certificate = [0; 32];
                remote.build_info.peer_build = [0; 32];
            }
            4 => remote.build_info.contract = [18; 32],
            _ => unreachable!(),
        }
        assert_admission(local.clone(), remote, false);
    }
}

#[test]
fn signed_different_versions_still_require_each_players_consent() {
    for local_consent in [false, true] {
        for remote_consent in [false, true] {
            assert_admission(
                certified(1, 2, 1, "1.0", local_consent),
                certified(2, 1, 2, "2.0", remote_consent),
                local_consent && remote_consent,
            );
        }
    }
}

#[test]
fn exact_artifact_fallback_survives_optional_pair_certificates() {
    let local = certified(1, 2, 1, "1.0", false);
    let mut ordinary = identity();
    ordinary.build = [1; 32];
    ordinary.build_info.version = "1.0".into();
    assert_admission(local.clone(), ordinary.clone(), true);
    assert_admission(ordinary, local.clone(), true);
    let mut changed_certificate = local.clone();
    changed_certificate.build_info.certificate = [32; 32];
    assert_admission(local, changed_certificate, true);
}

#[test]
fn signed_pair_refusal_emits_no_authentication_or_ready() {
    let (host, mut client) = streams();
    client.set_read_timeout(Some(IO_BUDGET)).unwrap();
    let local = certified(1, 2, 1, "1.0", false);
    let remote = certified(3, 1, 2, "1.0", false);
    let worker = thread::spawn(move || admit(host, Player::One, &local, &[9; 32]));
    client
        .write_all(&hello(Player::Two, &remote).unwrap())
        .unwrap();
    client.read_exact(&mut [0; HELLO_LEN]).unwrap();
    assert!(
        worker
            .join()
            .unwrap()
            .err()
            .unwrap()
            .to_string()
            .contains("expected peer")
    );
    assert_eq!(client.read(&mut [0]).unwrap(), 0);
}

#[test]
fn different_versions_require_both_consent_even_for_equal_artifacts() {
    for remote_build in [1, 2] {
        for local_consent in [false, true] {
            for remote_consent in [false, true] {
                assert_admission(
                    qualified(1, 1, "1.0", local_consent),
                    qualified(remote_build, 1, "2.0", remote_consent),
                    local_consent && remote_consent,
                );
            }
        }
    }
}

#[test]
fn consent_cannot_override_missing_or_changed_contract() {
    let local = qualified(1, 1, "1.0", true);
    for contract in [[0; 32], [18; 32]] {
        let mut remote = qualified(2, 2, "2.0", true);
        remote.build_info.contract = contract;
        if contract == [0; 32] {
            remote.build_info.platform = 0;
        }
        assert_admission(local.clone(), remote, false);
    }
    assert_admission(identity(), qualified(2, 2, "2.0", true), false);
}

#[test]
fn exact_artifacts_require_consistent_contract_and_platform_declarations() {
    let local = qualified(1, 1, "1.0", true);
    for remote in [qualified(1, 2, "1.0", true), identity()] {
        assert_admission(local.clone(), remote, false);
    }
    let mut remote = local.clone();
    remote.build_info.contract[0] ^= 1;
    assert_admission(local, remote, false);
}

#[test]
fn contract_and_consent_refusals_emit_no_authentication_or_ready() {
    for mismatch in [false, true] {
        let (host, mut client) = streams();
        client.set_read_timeout(Some(IO_BUDGET)).unwrap();
        let local = qualified(1, 1, "1.0", true);
        let mut remote = qualified(2, 2, "2.0", mismatch);
        if mismatch {
            remote.build_info.contract[0] ^= 1;
        }
        let worker = thread::spawn(move || admit(host, Player::One, &local, &[9; 32]));
        client
            .write_all(&hello(Player::Two, &remote).unwrap())
            .unwrap();
        client.read_exact(&mut [0; HELLO_LEN]).unwrap();
        let error = worker.join().unwrap().err().unwrap().to_string();
        assert!(error.contains(if mismatch { "contract" } else { "consent" }));
        assert_eq!(client.read(&mut [0]).unwrap(), 0);
    }
}

#[test]
fn qualified_artifacts_keep_every_session_identity_field_exact() {
    let local = qualified(1, 1, "1.0", true);
    for field in 0..7 {
        let mut remote = qualified(2, 2, "2.0", true);
        match field {
            0 => remote.source[0] ^= 1,
            1 => remote.effective[0] ^= 1,
            2 => remote.media_len += 1,
            3 => remote.config[0] ^= 1,
            4 => remote.initial[0] ^= 1,
            5 => remote.persistent[0] ^= 1,
            6 => remote.state_format += 1,
            _ => unreachable!(),
        }
        assert_admission(local.clone(), remote, false);
    }
}

#[test]
fn malformed_build_descriptors_reject_before_authentication_or_ready() {
    let valid = qualified(2, 2, "1.0", false);
    for (offset, value) in [
        (BUILD_INFO_OFFSET, 65),
        (BUILD_INFO_OFFSET + 1, 0),
        (BUILD_INFO_OFFSET + 1, b' '),
        (BUILD_INFO_OFFSET + 1, 0x80),
        (BUILD_INFO_OFFSET + 4, 1),
        (BUILD_INFO_OFFSET + 97, 0),
        (BUILD_INFO_OFFSET + 97, 3),
        (BUILD_INFO_OFFSET + 98, 2),
        (BUILD_INFO_OFFSET + 99, 1),
        (BUILD_INFO_OFFSET + 131, 1),
        (5, 2),
        (5, 3),
        (5, 4),
    ] {
        let (host, mut client) = streams();
        client.set_read_timeout(Some(IO_BUDGET)).unwrap();
        let local = qualified(1, 1, "1.0", false);
        let worker = thread::spawn(move || admit(host, Player::One, &local, &[9; 32]));
        let mut remote = hello(Player::Two, &valid).unwrap();
        remote[offset] = value;
        client.write_all(&remote).unwrap();
        let mut local_hello = [0; HELLO_LEN];
        client.read_exact(&mut local_hello).unwrap();
        assert!(worker.join().unwrap().is_err(), "offset {offset}");
        assert_eq!(client.read(&mut [0]).unwrap(), 0, "offset {offset}");
    }
}

#[test]
fn zero_contract_cannot_claim_platform_or_empty_qualified_version() {
    let mut remote = qualified(2, 2, "1.0", true);
    remote.build_info.contract = [0; 32];
    assert_admission(qualified(1, 1, "1.0", true), remote, false);
    let mut remote = qualified(2, 2, "", true);
    remote.build_info.allow_different_versions = false;
    assert_admission(qualified(1, 1, "1.0", true), remote, false);
}

#[test]
fn authenticated_hello_binds_artifact_version_contract_platform_and_consent() {
    let (host, mut client) = streams();
    let local = qualified(1, 1, "1.0", true);
    let remote = qualified(2, 2, "2.0", true);
    let worker = thread::spawn(move || admit(host, Player::One, &local, &[9; 32]));
    let sent = hello(Player::Two, &remote).unwrap();
    client.write_all(&sent).unwrap();
    let mut received = [0; HELLO_LEN];
    client.read_exact(&mut received).unwrap();
    let auth = tag(&[9; 32], &[AUTH_DOMAIN, &received, &sent, &[2]]);
    let transcript: [u8; 32] = Sha256::new()
        .chain_update(AUTH_DOMAIN)
        .chain_update(received)
        .chain_update(sent)
        .finalize()
        .into();
    for offset in [
        39,
        BUILD_INFO_OFFSET + 1,
        BUILD_INFO_OFFSET + 65,
        BUILD_INFO_OFFSET + 97,
        BUILD_INFO_OFFSET + 98,
        BUILD_INFO_OFFSET + 99,
        BUILD_INFO_OFFSET + 131,
    ] {
        let mut changed = sent;
        changed[offset] ^= 1;
        assert!(verify(&[9; 32], &[AUTH_DOMAIN, &received, &changed, &[2]], &auth).is_err());
        let changed_transcript: [u8; 32] = Sha256::new()
            .chain_update(AUTH_DOMAIN)
            .chain_update(received)
            .chain_update(changed)
            .finalize()
            .into();
        assert_ne!(transcript, changed_transcript);
    }
    client.write_all(&auth).unwrap();
    let mut host_auth = [0; TAG_LEN];
    client.read_exact(&mut host_auth).unwrap();
    verify(&[9; 32], &[AUTH_DOMAIN, &received, &sent, &[1]], &host_auth).unwrap();
    let ready = tag(&[9; 32], &[READY_DOMAIN, &transcript, &[2]]);
    client.write_all(&ready).unwrap();
    let mut host_ready = [0; TAG_LEN];
    client.read_exact(&mut host_ready).unwrap();
    verify(&[9; 32], &[READY_DOMAIN, &transcript, &[1]], &host_ready).unwrap();
    let connection = worker.join().unwrap().unwrap();
    assert_eq!(connection.transcript(), transcript);
}

#[test]
fn changed_hello_field_cannot_reuse_an_otherwise_valid_authentication() {
    for offset in [
        BUILD_INFO_OFFSET + 1,
        BUILD_INFO_OFFSET + 65,
        BUILD_INFO_OFFSET + 97,
        BUILD_INFO_OFFSET + 98,
        BUILD_INFO_OFFSET + 99,
        BUILD_INFO_OFFSET + 131,
    ] {
        let (host, mut client) = streams();
        let local = qualified(1, 1, "1.0", true);
        let remote = qualified(1, 1, "1.0", true);
        let worker = thread::spawn(move || admit(host, Player::One, &local, &[9; 32]));
        let sent = hello(Player::Two, &remote).unwrap();
        client.write_all(&sent).unwrap();
        let mut received = [0; HELLO_LEN];
        client.read_exact(&mut received).unwrap();
        let mut changed = sent;
        changed[offset] ^= 1;
        let auth = tag(&[9; 32], &[AUTH_DOMAIN, &received, &changed, &[2]]);
        client.write_all(&auth).unwrap();
        let mut host_auth = [0; TAG_LEN];
        client.read_exact(&mut host_auth).unwrap();
        assert!(worker.join().unwrap().is_err());
        assert_eq!(client.read(&mut [0]).unwrap(), 0);
    }
}
