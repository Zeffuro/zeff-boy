use super::*;

fn pack(path: &Path, entries: &[(&str, &[u8])]) -> Result<()> {
    let mut writer = zip::ZipWriter::new(File::create(path)?);
    for (name, bytes) in entries {
        writer.start_file(
            *name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )?;
        writer.write_all(bytes)?;
    }
    writer.finish()?;
    Ok(())
}

#[test]
fn invalid_pack_preflight_preserves_prior_entries_and_allows_the_next_song() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("all.zip");
    let temporary = directory.path().join("song.zip");
    let cancel = AtomicBool::new(false);
    let song = PlannedSong {
        id: SongId::Gbass(0),
        engine: "gbass".into(),
        ordinal: 0,
        title: "First song".into(),
        options: Some(Default::default()),
    };
    let mut file = File::create(&path)?;
    let mut archive = Archive::new(&mut file);
    archive.add_bytes("gbass/shared.gsflib", b"original library", &cancel)?;
    for entries in [
        vec![("new.gsflib", b"new".as_slice()), ("manifest.json", b"{}")],
        vec![
            ("new.gsflib", b"new".as_slice()),
            ("song.minigsf", b"mini"),
            ("unexpected.txt", b"bad"),
        ],
        vec![
            ("shared.gsflib", b"conflict".as_slice()),
            ("song.minigsf", b"mini"),
            ("manifest.json", b"{}"),
        ],
    ] {
        pack(&temporary, &entries)?;
        assert!(PreparedPack::read(&archive, &temporary, &song, &cancel).is_err());
        assert_eq!(archive.files.len(), 1);
    }
    let valid = [
        ("shared.gsflib", b"original library".as_slice()),
        ("song.minigsf", b"mini"),
        ("manifest.json", b"{}"),
    ];
    pack(&temporary, &valid)?;
    let manifest_offset = {
        let mut zip = zip::ZipArchive::new(File::open(&temporary)?)?;
        zip.by_name("manifest.json")?.data_start().unwrap() as usize
    };
    let mut corrupted = std::fs::read(&temporary)?;
    corrupted[manifest_offset] ^= 1;
    std::fs::write(&temporary, corrupted)?;
    assert!(PreparedPack::read(&archive, &temporary, &song, &cancel).is_err());
    assert_eq!(archive.files.len(), 1);
    pack(&temporary, &valid)?;
    let added =
        PreparedPack::read(&archive, &temporary, &song, &cancel)?.append(&mut archive, &cancel)?;
    assert_eq!(added.len(), 3);
    archive.finish()?;
    drop(file);
    let mut output = zip::ZipArchive::new(File::open(path)?)?;
    assert_eq!(output.len(), 3);
    assert!(output.by_name("gbass/new.gsflib").is_err());
    let mut library = Vec::new();
    output
        .by_name("gbass/shared.gsflib")?
        .read_to_end(&mut library)?;
    assert_eq!(library, b"original library");
    Ok(())
}

#[test]
fn largest_supported_native_pack_passes_preflight() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("song.zip");
    let output = directory.path().join("all.zip");
    let mut file = File::create(output)?;
    let archive = Archive::new(&mut file);
    let cancel = AtomicBool::new(false);
    let song = PlannedSong {
        id: SongId::Gbass(0),
        engine: "gbass".into(),
        ordinal: 0,
        title: "Song".into(),
        options: Some(Default::default()),
    };
    let mut names: Vec<_> = (0..super::super::super::gsf::MAX_NATIVE_PATCHES)
        .map(|index| format!("patch-{index}.gsflib"))
        .collect();
    names.extend(["song.minigsf".into(), "manifest.json".into()]);
    let entries: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), b"data".as_slice()))
        .collect();
    pack(&path, &entries)?;
    let prepared = PreparedPack::read(&archive, &path, &song, &cancel)?;
    assert_eq!(prepared.entries.len(), 66);
    names.push("extra.gsflib".into());
    let entries: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), b"data".as_slice()))
        .collect();
    pack(&path, &entries)?;
    assert!(PreparedPack::read(&archive, &path, &song, &cancel).is_err());
    Ok(())
}
