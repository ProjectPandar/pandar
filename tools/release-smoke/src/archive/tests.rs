use std::{fs, fs::File, io::Cursor, path::Path};

use flate2::{Compression, write::GzEncoder};
use tar::{Builder, Header};
use tempfile::tempdir;

use super::{normalized_top_level_path, stage_archive, validate_checksum};

#[test]
fn checksum_validation_rejects_digest_mismatch() {
    let temp = tempdir().unwrap();
    let archive = temp.path().join("archive.tar.gz");
    let checksum = temp.path().join("archive.tar.gz.sha256");
    fs::write(&archive, b"archive bytes").unwrap();
    fs::write(&checksum, format!("{} archive.tar.gz\n", "0".repeat(64))).unwrap();

    assert!(validate_checksum(&archive, &checksum).is_err());
}

#[test]
fn path_normalization_accepts_top_level_and_rejects_traversal() {
    assert_eq!(
        normalized_top_level_path(Path::new("./pandar")).unwrap(),
        Some("pandar".into())
    );
    assert!(normalized_top_level_path(Path::new("nested/pandar")).is_err());
    assert!(normalized_top_level_path(Path::new("../pandar")).is_err());
}

#[test]
fn staged_archive_rejects_duplicate_normalized_member_names() {
    let temp = tempdir().unwrap();
    let archive = temp.path().join("archive.tar.gz");
    create_tar_gz(
        &archive,
        &[
            ("pandar", b"first cli"),
            ("./pandar", b"second cli"),
            ("libpandar_network_plugin.so", b"plugin"),
            ("libpandar_bambu_source.so", b"source"),
        ],
    );

    let error = stage_archive(
        &archive,
        "pandar",
        "libpandar_network_plugin.so",
        "libpandar_bambu_source.so",
    )
    .err()
    .expect("duplicate normalized names must be rejected");

    assert_eq!(error, "archive contains duplicate normalized entry: pandar");
}

#[test]
fn staged_archive_rejects_case_folded_normalized_member_collisions() {
    let temp = tempdir().unwrap();
    let archive = temp.path().join("archive.tar.gz");
    create_tar_gz(
        &archive,
        &[
            ("pandar", b"first cli"),
            ("PANDAR", b"second cli"),
            ("libpandar_network_plugin.so", b"plugin"),
            ("libpandar_bambu_source.so", b"source"),
        ],
    );

    let error = stage_archive(
        &archive,
        "pandar",
        "libpandar_network_plugin.so",
        "libpandar_bambu_source.so",
    )
    .err()
    .expect("case-folded normalized names must not collide");

    assert_eq!(
        error,
        "archive contains case-folded normalized entry collision: PANDAR conflicts with pandar"
    );
}

fn create_tar_gz(path: &Path, entries: &[(&str, &[u8])]) {
    let file = File::create(path).unwrap();
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = Builder::new(encoder);
    for (name, content) in entries {
        let mut header = Header::new_gnu();
        header.set_mode(0o755);
        header.set_size(content.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, *name, Cursor::new(*content))
            .unwrap();
    }
    builder.finish().unwrap();
}
