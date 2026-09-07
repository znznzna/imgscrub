//! 全フィクスチャに対して成立しなければならない不変条件。
//!
//! これらは機能ではなく保証である。後から足しても意味がないため機能実装より先に置いている。

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use imgscrub::jpeg::filter::Options;
use imgscrub::jpeg::segment::{scan, scan_data, App};
use imgscrub::scrub::{process, process_file, Destination, Error};

fn tmp(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("invariants");
    fs::create_dir_all(&d).unwrap();
    let p = d.join(name);
    let _ = fs::remove_file(&p);
    p
}

fn sos(data: &[u8]) -> Vec<u8> {
    let segs = scan(data).unwrap();
    scan_data(data, &segs).to_vec()
}

/// 不変条件 1: スキャンデータが入力と完全一致する。
///
/// デコードせずに画素の無劣化を保証する唯一の方法。これが通る限り再エンコードは起きていない。
#[test]
fn scan_data_is_never_touched() {
    for name in common::ALL {
        let d = common::fixture(name);
        for opts in [Options { c2pa_only: true }, Options { c2pa_only: false }] {
            let (out, _) = process(&d, &opts).unwrap();
            assert_eq!(
                sos(&d),
                sos(&out),
                "{name} (c2pa_only={}) でスキャンデータが変わった",
                opts.c2pa_only
            );
        }
    }
}

/// 不変条件 2: `--c2pa-only` では APP11 以外の全セグメントがバイト単位で一致する。
#[test]
fn c2pa_only_touches_nothing_else() {
    for name in common::ALL {
        let d = common::fixture(name);
        let (out, _) = process(&d, &Options { c2pa_only: true }).unwrap();

        let keep: Vec<Vec<u8>> = scan(&d)
            .unwrap()
            .iter()
            .filter(|s| s.app_kind(&d) != Some(App::Jumbf))
            .map(|s| s.bytes(&d).to_vec())
            .collect();
        let got: Vec<Vec<u8>> = scan(&out)
            .unwrap()
            .iter()
            .map(|s| s.bytes(&out).to_vec())
            .collect();

        assert_eq!(keep, got, "{name}: APP11 以外が改変された");
    }
}

/// 不変条件 3: 冪等。2 回適用しても 1 回目と同一バイト列になる。
#[test]
fn processing_is_idempotent() {
    for name in common::ALL {
        let d = common::fixture(name);
        for opts in [Options { c2pa_only: true }, Options { c2pa_only: false }] {
            let (once, _) = process(&d, &opts).unwrap();
            let (twice, report) = process(&once, &opts).unwrap();
            assert_eq!(once, twice, "{name} が冪等でない");
            assert!(
                !report.changed(),
                "{name}: 2 回目に変更が発生した {:?}",
                report.removals
            );
        }
    }
}

/// 不変条件 4: 出力が JPEG として再走査でき、EOI で終わる。
#[test]
fn output_is_a_valid_jpeg() {
    for name in common::ALL {
        let d = common::fixture(name);
        let (out, _) = process(&d, &Options::default()).unwrap();
        assert!(scan(&out).is_ok(), "{name}: 出力を再走査できない");
        assert!(out.ends_with(b"\xff\xd9"), "{name}: EOI で終わっていない");
        assert!(out.starts_with(b"\xff\xd8"), "{name}: SOI で始まっていない");
    }
}

/// 不変条件 5: 異常系で入力ファイルが変更されない。
#[test]
fn broken_input_is_left_untouched() {
    let src = common::path("truncated.jpg");
    let work = tmp("truncated.jpg");
    fs::copy(&src, &work).unwrap();
    let before = fs::read(&work).unwrap();

    let err = process_file(&work, &Destination::InPlace, &Options::default(), false)
        .expect_err("切り詰めファイルはエラーになる");
    assert!(matches!(err, Error::Jpeg(_)), "予期しないエラー: {err}");

    assert_eq!(before, fs::read(&work).unwrap(), "入力が変更された");
}

/// 不変条件 6: dry-run はファイルを作らない・変更しない。
#[test]
fn dry_run_writes_nothing() {
    let work = tmp("dry.jpg");
    fs::write(&work, common::fixture("lrc_firefly.jpg")).unwrap();
    let before = fs::read(&work).unwrap();

    let (report, out_path) =
        process_file(&work, &Destination::Sibling, &Options::default(), true).unwrap();

    assert!(report.changed(), "dry-run でも変更内容は報告される");
    assert_eq!(before, fs::read(&work).unwrap(), "入力が変更された");
    assert!(!out_path.exists(), "dry-run で出力が作られた");
}

/// 既定の出力先は入力の隣に `_clean` を付けた名前になる。
#[test]
fn sibling_destination_naming() {
    let d = Destination::Sibling;
    assert_eq!(
        d.resolve(Path::new("/a/b/photo.jpg")),
        PathBuf::from("/a/b/photo_clean.jpg")
    );
    assert_eq!(
        d.resolve(Path::new("/a/b/photo.JPEG")),
        PathBuf::from("/a/b/photo_clean.JPEG")
    );
}

/// C2PA を持つファイルからは確実に APP11 が消え、持たないファイルは報告が空になる。
#[test]
fn c2pa_removal_is_reported_accurately() {
    let d = common::fixture("lrc_firefly.jpg");
    let (out, report) = process(&d, &Options { c2pa_only: true }).unwrap();
    assert_eq!(report.removals.len(), 1);
    assert_eq!(report.removals[0].bytes, 14_478);
    assert!(!scan(&out)
        .unwrap()
        .iter()
        .any(|s| s.app_kind(&out) == Some(App::Jumbf)));

    let d = common::fixture("lrc_clean.jpg");
    let (_, report) = process(&d, &Options { c2pa_only: true }).unwrap();
    assert!(!report.changed(), "C2PA がないファイルは無変更: {report:?}");
}

/// MPF は先行セグメントを削除した時だけ削除される（設計書 §4.3）。
#[test]
fn mpf_dropped_only_when_offsets_break() {
    let d = common::fixture("camera_mpf.jpg");

    // このフィクスチャは APP11 を持たないので、--c2pa-only では何も削除されない。
    // よって MPF のオフセットは保たれ、MPF も残る。
    let (out, report) = process(&d, &Options { c2pa_only: true }).unwrap();
    assert!(!report.changed());
    assert!(scan(&out)
        .unwrap()
        .iter()
        .any(|s| s.app_kind(&out) == Some(App::Mpf)));

    // APP11 を先頭に足すと、その削除で MPF のオフセットが壊れるため MPF も削除される。
    let firefly = common::fixture("lrc_firefly.jpg");
    let fsegs = scan(&firefly).unwrap();
    let jumbf = fsegs
        .iter()
        .find(|s| s.app_kind(&firefly) == Some(App::Jumbf))
        .unwrap();
    let mut spliced = Vec::new();
    spliced.extend_from_slice(&d[..2]);
    spliced.extend_from_slice(jumbf.bytes(&firefly));
    spliced.extend_from_slice(&d[2..]);

    let (out, report) = process(&spliced, &Options { c2pa_only: true }).unwrap();
    assert_eq!(
        report.removals.len(),
        2,
        "APP11 と MPF: {:?}",
        report.removals
    );
    assert!(!scan(&out)
        .unwrap()
        .iter()
        .any(|s| s.app_kind(&out) == Some(App::Mpf)));
}
