use super::*;
use crate::test_support::test_directory;

fn identity(prefix: u8, cursor: u64) -> TasSeekCacheIdentity {
    TasSeekCacheIdentity {
        cache_format_version: 1,
        state_format_compatibility_id: "nes-native-v11".to_owned(),
        sync_identity_sha256: TasDigest([0xA1; 32]),
        branch_prefix_sha256: TasDigest([prefix; 32]),
        cursor,
    }
}

#[test]
fn stores_at_a_deterministic_content_keyed_path_and_verifies_state_sha() {
    let root = test_directory("tas-seek-cache").unwrap();
    let cache = TasSeekStateCache::open(root.path()).unwrap();
    let identity = identity(0x10, 6);
    let path = cache.path_for(&identity).unwrap();

    cache.store(&identity, b"synthetic state").unwrap();
    assert_eq!(cache.path_for(&identity).unwrap(), path);
    assert_eq!(
        cache.load(&identity).unwrap(),
        Some(b"synthetic state".to_vec())
    );

    let mut bytes = fs::read(&path).unwrap();
    *bytes.last_mut().unwrap() ^= 0x01;
    fs::write(&path, bytes).unwrap();
    assert_eq!(cache.load(&identity).unwrap(), None);
    assert!(!path.exists());
}

#[test]
fn identity_mismatch_is_disposable() {
    let root = test_directory("tas-seek-cache-mismatch").unwrap();
    let cache = TasSeekStateCache::open(root.path()).unwrap();
    let stored = identity(0x20, 4);
    let requested = identity(0x20, 5);
    let requested_path = cache.path_for(&requested).unwrap();
    let bytes = encode_entry(&stored, b"state", MAX_SEEK_CACHE_STATE_BYTES).unwrap();
    fs::write(&requested_path, bytes).unwrap();

    assert_eq!(cache.load(&requested).unwrap(), None);
    assert!(!requested_path.exists());
}

#[test]
fn shared_prefix_reuses_a_state_but_an_edit_at_n_rejects_later_cursor() {
    let root = test_directory("tas-seek-cache-prefix").unwrap();
    let cache = TasSeekStateCache::open(root.path()).unwrap();
    let before_edit = identity(0x31, 12);
    let shared_branch = identity(0x31, 12);
    cache.store(&before_edit, b"cursor before frame N").unwrap();
    assert_eq!(
        cache.load(&shared_branch).unwrap(),
        Some(b"cursor before frame N".to_vec())
    );

    let after_edit_before_n = identity(0x31, 12);
    let before_edit_after_n = identity(0x41, 13);
    let after_edit_after_n = identity(0x42, 13);
    cache
        .store(&before_edit_after_n, b"old post-edit-prefix state")
        .unwrap();
    assert_eq!(
        cache.load(&after_edit_before_n).unwrap(),
        Some(b"cursor before frame N".to_vec())
    );
    assert_eq!(cache.load(&after_edit_after_n).unwrap(), None);
}

#[test]
fn newest_matching_scan_is_bounded_by_entries_and_skips_foreign_or_corrupt_states() {
    let root = test_directory("tas-seek-cache-newest").unwrap();
    let cache = TasSeekStateCache::open(root.path()).unwrap();
    cache.store(&identity(3, 3), b"cursor three").unwrap();
    cache.store(&identity(7, 7), b"cursor seven").unwrap();
    cache
        .store(&identity(9, 9), b"foreign cursor nine")
        .unwrap();

    let newest = cache
        .load_newest_matching(8, |cursor| Ok(identity(cursor as u8, cursor)))
        .unwrap();
    assert_eq!(newest, Some((7, b"cursor seven".to_vec())));

    let corrupt_path = cache.path_for(&identity(7, 7)).unwrap();
    let mut corrupt = fs::read(&corrupt_path).unwrap();
    *corrupt.last_mut().unwrap() ^= 1;
    fs::write(&corrupt_path, corrupt).unwrap();
    let newest = cache
        .load_newest_matching(8, |cursor| Ok(identity(cursor as u8, cursor)))
        .unwrap();
    assert_eq!(newest, Some((3, b"cursor three".to_vec())));
    assert!(!corrupt_path.exists());
    assert!(cache.path_for(&identity(9, 9)).unwrap().exists());
}

#[test]
fn newest_matching_bounds_project_prefix_identity_work() {
    let root = test_directory("tas-seek-cache-identity-budget").unwrap();
    let cache = TasSeekStateCache::open(root.path()).unwrap();
    for cursor in 1..=12 {
        cache
            .store(&identity(cursor as u8, cursor), &[cursor as u8])
            .unwrap();
    }

    let checks = std::cell::Cell::new(0usize);
    let newest = cache
        .load_newest_matching(12, |cursor| {
            checks.set(checks.get() + 1);
            Ok(identity(0xFF, cursor))
        })
        .unwrap();

    assert_eq!(newest, None);
    assert_eq!(checks.get(), MAX_SEEK_CACHE_IDENTITY_CHECKS_PER_LOAD);
}

#[test]
fn pruning_is_bounded_and_never_removes_a_sibling_outside_the_exact_root() {
    let parent = test_directory("tas-seek-cache-prune").unwrap();
    let root = parent.path().join("cache");
    let outside = parent.path().join("outside.zsc");
    fs::write(&outside, b"outside").unwrap();
    let cache = TasSeekStateCache::open_with_limits(
        &root,
        TasSeekCacheLimits {
            max_entries: 2,
            max_bytes: 1024,
            max_state_bytes: 128,
        },
    )
    .unwrap();

    cache.store(&identity(1, 1), &[1; 32]).unwrap();
    cache.store(&identity(2, 2), &[2; 32]).unwrap();
    cache.store(&identity(3, 3), &[3; 32]).unwrap();

    assert!(outside.exists());
    assert!(cache.collect_entries().unwrap().len() <= 2);
    assert!(
        cache
            .collect_entries()
            .unwrap()
            .iter()
            .map(|entry| entry.bytes)
            .sum::<u64>()
            <= 1024
    );
}

#[test]
fn cache_root_cannot_be_inside_a_project_package() {
    let root = test_directory("tas-seek-cache-ztas").unwrap();
    let project = root.path().join("project.ztas");
    assert!(TasSeekStateCache::open(project.join("cache")).is_err());
}

#[test]
fn directory_scan_is_bounded() {
    let root = test_directory("tas-seek-cache-scan-limit").unwrap();
    let cache = TasSeekStateCache::open(root.path()).unwrap();
    for index in 0..=MAX_SEEK_CACHE_DIRECTORY_ENTRIES {
        fs::write(root.path().join(format!("unrelated-{index}")), b"").unwrap();
    }

    assert!(cache.prune().is_err());
}

#[cfg(not(windows))]
#[test]
fn replaced_root_is_rejected_before_read_write_or_prune() {
    let parent = test_directory("tas-seek-cache-root-replacement").unwrap();
    let root = parent.path().join("cache");
    let cache = TasSeekStateCache::open(&root).unwrap();
    let displaced = parent.path().join("displaced-cache");
    fs::rename(&root, &displaced).unwrap();
    fs::create_dir(&root).unwrap();

    let identity = identity(0x71, 9);
    assert!(cache.load(&identity).is_err());
    assert!(
        cache
            .store(&identity, b"must not reach replacement")
            .is_err()
    );
    assert!(cache.prune().is_err());
    assert!(fs::read_dir(&root).unwrap().next().is_none());
}

#[cfg(windows)]
#[test]
fn stable_directory_guard_allows_normal_cache_operations() {
    let root = test_directory("tas-seek-cache-windows-root-guard").unwrap();
    let cache = TasSeekStateCache::open(root.path()).unwrap();
    let identity = identity(0x73, 11);
    cache.store(&identity, b"guarded state").unwrap();
    assert_eq!(
        cache.load(&identity).unwrap(),
        Some(b"guarded state".to_vec())
    );
    cache.prune().unwrap();
}

#[cfg(windows)]
#[test]
fn case_equivalent_cache_filename_is_rejected_before_accounting() {
    let root = test_directory("tas-seek-cache-case-equivalent").unwrap();
    let cache = TasSeekStateCache::open(root.path()).unwrap();
    let identity = identity(0x72, 10);
    let canonical = cache.path_for(&identity).unwrap();
    let upper = canonical
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_ascii_uppercase();
    fs::write(root.path().join(upper), b"not a valid cache entry").unwrap();

    assert!(cache.load(&identity).is_err());
    assert!(cache.prune().is_err());
    assert!(
        cache
            .store(&identity, b"must not overwrite case-equivalent")
            .is_err()
    );
}
