use super::*;

#[path = "loading/rom.rs"]
mod rom;

pub(crate) use rom::load_7z_rom_entry_with_control;

#[derive(Clone, Copy)]
pub(super) struct DecodePassPolicy {
    pub(super) progress_base: u64,
    pub(super) decoded_bytes_limit: u64,
}

pub(crate) enum SevenZipContents {
    Cd { cue_path: PathBuf },
    Roms(Vec<ArchiveRomEntry>),
}

pub(crate) fn inspect_7z_cue_members(
    path: &Path,
    decoder_memory_limit_mib: usize,
) -> Result<Vec<String>, PceCdLoadError> {
    let (_, manifest) = open_validated(path, decoder_memory_limit_mib)?;
    Ok(manifest.cue_names)
}

pub(crate) fn inspect_7z_cue_candidates_with_archive_identity(
    path: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
) -> Result<Vec<PceCdArchiveCueCandidate>, PceCdLoadError> {
    check_cancelled(cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::Inspecting);
    let (_, manifest, source) =
        open_validated_with_source_fingerprint(path, decoder_memory_limit_mib, cancel, progress)?;
    manifest
        .cue_names
        .into_iter()
        .map(|cue_member| {
            Ok(PceCdArchiveCueCandidate {
                identity: archive_cue_identity(
                    source,
                    &cue_member,
                    PceCdArchiveCueSelection::Explicit,
                )?,
                cue_member,
            })
        })
        .collect()
}

pub(crate) fn inspect_7z_ppf_candidates_with_archive_identity(
    path: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
) -> Result<Vec<PceCdArchivePpfCandidate>, PceCdLoadError> {
    check_cancelled(cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::Inspecting);
    let (mut reader, manifest, source, mut source_verifier) =
        open_validated_with_source_verifier(path, decoder_memory_limit_mib, cancel, progress)?;
    let descriptors = ppf_members(&manifest);
    let mut candidates = Vec::with_capacity(manifest.cue_names.len());
    let mut targets = BTreeSet::new();
    for cue_name in &manifest.cue_names {
        if let Some(names) = ppf::discover_archive_ppf_members(cue_name, &descriptors)? {
            targets.extend(names.iter().cloned());
            candidates.push((cue_name.clone(), names));
        }
    }
    if candidates.is_empty() {
        return Err(PceCdLoadError::NoArchivePpfStack);
    }
    let mut extracted = decode_pass(
        &mut reader,
        &manifest,
        &targets,
        cancel,
        progress,
        DecodePassPolicy {
            progress_base: 0,
            decoded_bytes_limit: PCE_CD_7Z_DECODED_BYTES_LIMIT,
        },
    )?;
    reauthenticate_source(&mut source_verifier, source, path, cancel, progress)?;
    candidates
        .into_iter()
        .map(|(cue_member, names)| {
            Ok(PceCdArchivePpfCandidate {
                identity: archive_cue_identity(
                    source,
                    &cue_member,
                    PceCdArchiveCueSelection::Explicit,
                )?,
                patches: ppf::patch_identities(&names, &mut extracted)?,
                cue_member,
            })
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn inspect_7z_cue_path(path: &Path) -> Result<PathBuf, PceCdLoadError> {
    let (_, manifest) = open_validated(path, DEFAULT_DECODER_MEMORY_LIMIT_MIB)?;
    Ok(virtual_member_path(path, unique_cue_name(&manifest)?))
}

pub(crate) fn inspect_7z_contents(
    path: &Path,
    decoder_memory_limit_mib: usize,
) -> Result<SevenZipContents, PceCdLoadError> {
    let (_, manifest) = open_validated(path, decoder_memory_limit_mib)?;
    if !manifest.cue_names.is_empty() {
        return Ok(SevenZipContents::Cd {
            cue_path: virtual_member_path(path, unique_cue_name(&manifest)?),
        });
    }
    let entries = rom_entries(&manifest);
    if entries.is_empty() {
        Err(PceCdLoadError::NoSupportedArchiveContent)
    } else {
        Ok(SevenZipContents::Roms(entries))
    }
}

#[cfg(test)]
pub(crate) fn load_7z_cue(path: &Path) -> Result<LoadedPceCd, PceCdLoadError> {
    let cancel = AtomicBool::new(false);
    let progress = PceCdPackageProgress::default();
    load_7z_cue_with_control(path, &cancel, &progress, DEFAULT_DECODER_MEMORY_LIMIT_MIB)
        .map(|(_, loaded)| loaded)
}

#[cfg(test)]
pub(crate) fn load_7z_cue_with_control(
    path: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
    load_7z_cue_with_control_and_mods(path, cancel, progress, decoder_memory_limit_mib, false)
}

pub(crate) fn load_7z_cue_with_control_and_mods(
    path: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
    apply_mods: bool,
) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
    let cache_root = pce_cd_cache_root();
    load_7z_cue_with_cache_root(
        path,
        cancel,
        progress,
        decoder_memory_limit_mib,
        apply_mods,
        &cache_root,
    )
}

pub(crate) fn load_7z_cue_with_control_and_archive_identity(
    path: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
    apply_mods: bool,
) -> Result<(PathBuf, LoadedPceCd, PceCdArchiveCueIdentity), PceCdLoadError> {
    let cache_root = pce_cd_cache_root();
    load_7z_cue_with_cache_root_and_archive_identity(
        path,
        CachedCueLoadOptions {
            cancel,
            progress,
            decoder_memory_limit_mib,
            apply_mods,
            cache_root: &cache_root,
            reuse_cache: false,
            selected_cue_name: None,
        },
    )
}

pub(crate) fn load_7z_selected_cue_with_control_and_archive_identity(
    path: &Path,
    selected_cue_name: &str,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
    apply_mods: bool,
) -> Result<(PathBuf, LoadedPceCd, PceCdArchiveCueIdentity), PceCdLoadError> {
    let cache_root = pce_cd_cache_root();
    load_7z_cue_with_cache_root_and_archive_identity(
        path,
        CachedCueLoadOptions {
            cancel,
            progress,
            decoder_memory_limit_mib,
            apply_mods,
            cache_root: &cache_root,
            reuse_cache: false,
            selected_cue_name: Some(selected_cue_name),
        },
    )
}

pub(crate) fn load_7z_cue_with_control_and_archive_ppf(
    path: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
) -> Result<PceCdArchivePpfLoad, PceCdLoadError> {
    load_7z_archive_ppf(path, None, cancel, progress, decoder_memory_limit_mib)
}

pub(crate) fn load_7z_selected_cue_with_control_and_archive_ppf(
    path: &Path,
    selected_cue_name: &str,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
) -> Result<PceCdArchivePpfLoad, PceCdLoadError> {
    load_7z_archive_ppf(
        path,
        Some(selected_cue_name),
        cancel,
        progress,
        decoder_memory_limit_mib,
    )
}

fn load_7z_archive_ppf(
    path: &Path,
    selected_cue_name: Option<&str>,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
) -> Result<PceCdArchivePpfLoad, PceCdLoadError> {
    let _cache_guard = CACHE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    check_cancelled(cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::Inspecting);
    let (mut reader, manifest, source, mut source_verifier) =
        open_validated_with_source_verifier(path, decoder_memory_limit_mib, cancel, progress)?;
    let cue_name = select_normalized_cue_name(&manifest.cue_names, selected_cue_name)?.to_owned();
    let patch_names = ppf::discover_archive_ppf_members(&cue_name, &ppf_members(&manifest))?
        .ok_or(PceCdLoadError::NoArchivePpfStack)?;
    let selection = if selected_cue_name.is_some() {
        PceCdArchiveCueSelection::Explicit
    } else {
        PceCdArchiveCueSelection::Unique
    };
    let archive_identity = archive_cue_identity(source, &cue_name, selection)?;
    validate_cacheable_manifest(&manifest)?;
    let cache_root = pce_cd_cache_root();
    std::fs::create_dir_all(&cache_root)
        .map_err(|error| PceCdLoadError::Archive(format!("CD cache: {error}")))?;
    let base_key = cache_key(path, source);
    static STRICT_PPF_CACHE_NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = STRICT_PPF_CACHE_NONCE.fetch_add(1, Ordering::Relaxed);
    let strict_key = format!("{base_key}-tas-ppf-{}-{nonce}", std::process::id());
    let identity = CacheIdentity {
        source,
        key: &strict_key,
    };
    let cache = extract_cache(
        &mut reader,
        &manifest,
        identity,
        &cache_root,
        cancel,
        progress,
    )?;
    let (mut loaded, patches) = match load_cached_archive_ppf_disc(
        &cache,
        &manifest,
        &cue_name,
        &patch_names,
        cancel,
        progress,
    ) {
        Ok(result) => result,
        Err(CachedDiscError::Load(error)) => return Err(error),
        Err(CachedDiscError::Corrupt) => {
            remove_cache_entry(&cache_root, &cache.path);
            return Err(PceCdLoadError::ArchiveChanged);
        }
    };
    if let Err(error) = reauthenticate_source(&mut source_verifier, source, path, cancel, progress)
    {
        remove_cache_entry(&cache_root, &cache.path);
        return Err(error);
    }
    let unpatched_disc_sha256 = loaded.source_disc_sha256;
    touch_cache_entry(&cache.path);
    prune_cache(&cache_root, Some(&cache.path));
    progress.set_phase(PceCdPackageLoadPhase::Complete);
    loaded.source_disc_sha256 = unpatched_disc_sha256;
    Ok(PceCdArchivePpfLoad {
        cue_path: virtual_member_path(path, &cue_name),
        loaded,
        archive_identity,
        patches,
        unpatched_disc_sha256,
    })
}

fn ppf_members(manifest: &ArchiveManifest) -> Vec<ppf::ArchivePpfMember> {
    manifest
        .entries
        .iter()
        .map(|member| ppf::ArchivePpfMember {
            name: member.name.clone(),
            size: member.size,
            is_regular: !member.is_directory
                && !member.is_anti
                && !member.is_link
                && member.has_stream
                && member.crc_checked,
        })
        .collect()
}

#[cfg(feature = "profile-cores")]
pub(crate) fn profile_cache_load(path: &Path, cache_root: &Path) -> Result<(), String> {
    std::fs::create_dir_all(cache_root).map_err(|error| error.to_string())?;
    if std::fs::read_dir(cache_root)
        .map_err(|error| error.to_string())?
        .next()
        .is_some()
    {
        return Err("ZEFF_PROFILE_PCE_CD_CACHE_ROOT must be empty".to_owned());
    }

    let legacy_started = Instant::now();
    let legacy = profile_legacy_load(path).map_err(|error| error.to_string())?;
    let legacy_elapsed = legacy_started.elapsed();

    let load = || {
        let cancel = AtomicBool::new(false);
        let progress = PceCdPackageProgress::default();
        let started = Instant::now();
        let loaded = load_7z_cue_with_cache_root(path, &cancel, &progress, 512, false, cache_root)
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((loaded, started.elapsed(), progress.total_bytes()))
    };

    let (cold, cold_elapsed, cold_bytes) = load()?;
    let (warm, warm_elapsed, warm_bytes) = load()?;
    if legacy.0 != cold.0
        || legacy.1.content_sha256 != cold.1.content_sha256
        || legacy.1.content_crc32 != cold.1.content_crc32
        || legacy.1.source_disc_sha256 != cold.1.source_disc_sha256
        || legacy.1.disc != cold.1.disc
        || cold.0 != warm.0
        || cold.1.content_sha256 != warm.1.content_sha256
        || cold.1.content_crc32 != warm.1.content_crc32
        || cold.1.source_disc_sha256 != warm.1.source_disc_sha256
        || cold.1.disc != warm.1.disc
    {
        return Err("cold and warm cache loads differ".to_owned());
    }

    println!(
        "pce_cd_cache legacy_ms={:.3} cold_ms={:.3} warm_ms={:.3} cold_progress_bytes={} warm_progress_bytes={}",
        legacy_elapsed.as_secs_f64() * 1_000.0,
        cold_elapsed.as_secs_f64() * 1_000.0,
        warm_elapsed.as_secs_f64() * 1_000.0,
        cold_bytes,
        warm_bytes,
    );
    Ok(())
}

#[cfg(feature = "profile-cores")]
fn profile_legacy_load(path: &Path) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
    let cancel = AtomicBool::new(false);
    let progress = PceCdPackageProgress::default();
    let (mut reader, manifest) = open_validated(path, 512)?;
    let cue_name = unique_cue_name(&manifest)?.to_owned();
    let decoded_per_pass = manifest
        .entries
        .iter()
        .try_fold(0_u64, |total, member| total.checked_add(member.size))
        .ok_or(PceCdLoadError::ArchiveDecodedLimit)?;
    let cue_target = BTreeSet::from([cue_name.clone()]);
    let mut cue_members = decode_pass(
        &mut reader,
        &manifest,
        &cue_target,
        &cancel,
        &progress,
        DecodePassPolicy {
            progress_base: 0,
            decoded_bytes_limit: PCE_CD_7Z_DECODED_BYTES_LIMIT,
        },
    )?;
    let cue_bytes = cue_members
        .remove(&cue_name)
        .ok_or_else(|| PceCdLoadError::ArchiveMemberMissing(cue_name.clone()))?;
    let sheet = parse_cue_bytes(&cue_bytes)?;

    let mut resolved = Vec::with_capacity(sheet.files.len());
    let mut targets = BTreeSet::from([cue_name.clone()]);
    for file in &sheet.files {
        let name = resolve_reference(&manifest, &cue_name, &file.reference)?;
        targets.insert(name.clone());
        resolved.push(name);
    }
    let mut members = decode_pass(
        &mut reader,
        &manifest,
        &targets,
        &cancel,
        &progress,
        DecodePassPolicy {
            progress_base: decoded_per_pass,
            decoded_bytes_limit: PCE_CD_7Z_DECODED_BYTES_LIMIT,
        },
    )?;
    let second_cue = members
        .remove(&cue_name)
        .ok_or(PceCdLoadError::ArchiveChanged)?;
    if second_cue != cue_bytes {
        return Err(PceCdLoadError::ArchiveChanged);
    }
    let files = resolved
        .into_iter()
        .map(|name| {
            members
                .remove(&name)
                .ok_or(PceCdLoadError::ArchiveMemberMissing(name))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let loaded = build_disc_with_mods(cue_bytes, &sheet, files, false)?;
    Ok((virtual_member_path(path, &cue_name), loaded))
}

pub(crate) fn load_7z_cue_with_cache_root(
    path: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
    apply_mods: bool,
    cache_root: &Path,
) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
    load_7z_cue_with_cache_root_and_archive_identity(
        path,
        CachedCueLoadOptions {
            cancel,
            progress,
            decoder_memory_limit_mib,
            apply_mods,
            cache_root,
            reuse_cache: true,
            selected_cue_name: None,
        },
    )
    .map(|(cue_path, loaded, _)| (cue_path, loaded))
}

struct CachedCueLoadOptions<'a> {
    cancel: &'a AtomicBool,
    progress: &'a PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
    apply_mods: bool,
    cache_root: &'a Path,
    reuse_cache: bool,
    selected_cue_name: Option<&'a str>,
}

fn load_7z_cue_with_cache_root_and_archive_identity(
    path: &Path,
    options: CachedCueLoadOptions<'_>,
) -> Result<(PathBuf, LoadedPceCd, PceCdArchiveCueIdentity), PceCdLoadError> {
    let CachedCueLoadOptions {
        cancel,
        progress,
        decoder_memory_limit_mib,
        apply_mods,
        cache_root,
        reuse_cache,
        selected_cue_name,
    } = options;
    let _cache_guard = CACHE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    check_cancelled(cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::Inspecting);
    let (mut reader, manifest, source) =
        open_validated_with_source_fingerprint(path, decoder_memory_limit_mib, cancel, progress)?;
    let cue_name = select_normalized_cue_name(&manifest.cue_names, selected_cue_name)?.to_owned();
    let archive_identity = archive_cue_identity(
        source,
        &cue_name,
        if selected_cue_name.is_some() {
            PceCdArchiveCueSelection::Explicit
        } else {
            PceCdArchiveCueSelection::Unique
        },
    )?;
    check_cancelled(cancel)?;
    validate_cacheable_manifest(&manifest)?;
    let decoded_bytes = manifest
        .entries
        .iter()
        .try_fold(0_u64, |total, member| total.checked_add(member.size))
        .ok_or(PceCdLoadError::ArchiveDecodedLimit)?;
    if decoded_bytes > CACHE_ENTRY_BYTES_LIMIT {
        return Err(PceCdLoadError::ArchiveDecodedLimit);
    }
    let cache_key = cache_key(path, source);
    static STRICT_CACHE_NONCE: AtomicU64 = AtomicU64::new(0);
    let strict_cache_key = (!reuse_cache).then(|| {
        let nonce = STRICT_CACHE_NONCE.fetch_add(1, Ordering::Relaxed);
        format!("{cache_key}-tas-{}-{nonce}", std::process::id())
    });
    let identity = CacheIdentity {
        source,
        key: strict_cache_key.as_deref().unwrap_or(&cache_key),
    };
    let mut cache = if reuse_cache {
        prepare_cache(
            &mut reader,
            &manifest,
            identity,
            cache_root,
            cancel,
            progress,
        )?
    } else {
        std::fs::create_dir_all(cache_root)
            .map_err(|error| PceCdLoadError::Archive(format!("CD cache: {error}")))?;
        extract_cache(
            &mut reader,
            &manifest,
            identity,
            cache_root,
            cancel,
            progress,
        )?
    };

    for attempt in 0..2 {
        match load_cached_disc(&cache, &manifest, &cue_name, cancel, progress, apply_mods) {
            Ok(loaded) => {
                touch_cache_entry(&cache.path);
                prune_cache(cache_root, Some(&cache.path));
                return Ok((
                    virtual_member_path(path, &cue_name),
                    loaded,
                    archive_identity,
                ));
            }
            Err(CachedDiscError::Load(error)) => return Err(error),
            Err(CachedDiscError::Corrupt) if attempt == 0 => {
                remove_cache_entry(cache_root, &cache.path);
                let (mut retry_reader, retry_manifest, retry_source) =
                    open_validated_with_source_fingerprint(
                        path,
                        decoder_memory_limit_mib,
                        cancel,
                        progress,
                    )?;
                if retry_manifest != manifest || retry_source != source {
                    return Err(PceCdLoadError::ArchiveChanged);
                }
                cache = extract_cache(
                    &mut retry_reader,
                    &manifest,
                    identity,
                    cache_root,
                    cancel,
                    progress,
                )?;
            }
            Err(CachedDiscError::Corrupt) => return Err(PceCdLoadError::ArchiveChanged),
        }
    }
    unreachable!()
}

#[cfg(test)]
pub(crate) fn load_7z_cue_with_cache_root_and_archive_identity_for_test(
    path: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
    cache_root: &Path,
) -> Result<(PathBuf, LoadedPceCd, PceCdArchiveCueIdentity), PceCdLoadError> {
    load_7z_cue_with_cache_root_and_archive_identity(
        path,
        CachedCueLoadOptions {
            cancel,
            progress,
            decoder_memory_limit_mib,
            apply_mods: false,
            cache_root,
            reuse_cache: false,
            selected_cue_name: None,
        },
    )
}
