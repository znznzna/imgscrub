//! セグメント走査のテスト。
//!
//! 期待値は設計書 §1.1 の実測値に基づく。フィクスチャは実写真から画素だけを 16x16 に
//! 縮小し、APPn は実物のバイト列を移植して作っている（tools/mkfixture.py）。

mod common;

use imgscrub::jpeg::segment::{check_eoi, scan, scan_data, App, JpegError, SOS};

/// APPn の種別を出現順に並べたもの。
fn app_kinds(data: &[u8]) -> Vec<App> {
    scan(data)
        .unwrap()
        .iter()
        .filter_map(|s| s.app_kind(data))
        .collect()
}

#[test]
fn firefly_fixture_matches_measured_structure() {
    let d = common::fixture("lrc_firefly.jpg");
    let segs = scan(&d).unwrap();

    // 設計書 §1.1 の構成。APP11 が先頭に来る
    assert_eq!(
        app_kinds(&d),
        vec![
            App::Jumbf,
            App::Exif,
            App::Photoshop,
            App::Icc,
            App::Xmp,
            App::Adobe
        ]
    );

    // C2PA マニフェストのサイズは実測値と一致する（移植なので変わらない）
    let jumbf = segs
        .iter()
        .find(|s| s.app_kind(&d) == Some(App::Jumbf))
        .expect("APP11/JUMBF がある");
    assert_eq!(jumbf.size(), 14_478);
    assert_eq!(jumbf.start, 2, "APP11 はファイル先頭に来る");

    // SOS は 1 つだけで、末尾まで伸びる
    let sos: Vec<_> = segs.iter().filter(|s| s.marker == SOS).collect();
    assert_eq!(sos.len(), 1);
    assert_eq!(sos[0].end, d.len());
}

#[test]
fn clean_fixture_has_no_c2pa() {
    let d = common::fixture("lrc_clean.jpg");
    let kinds = app_kinds(&d);

    // 同一 Lightroom バージョンでも Firefly を通していなければ APP11 は存在しない
    // （設計書 §1.1.1 の対照実験）
    assert!(!kinds.contains(&App::Jumbf), "C2PA がないこと: {kinds:?}");
    assert_eq!(
        kinds,
        vec![App::Exif, App::Photoshop, App::Icc, App::Xmp, App::Adobe]
    );
}

#[test]
fn mpf_fixture_is_detected() {
    let d = common::fixture("camera_mpf.jpg");
    // MPF は絶対オフセットを含むため、先行セグメントを削除すると破綻する（設計書 §4.3）
    assert!(app_kinds(&d).contains(&App::Mpf));
}

#[test]
fn minimal_jpeg_scans() {
    let d = common::fixture("no_exif.jpg");
    let segs = scan(&d).unwrap();
    assert!(segs.iter().any(|s| s.marker == SOS));
    assert!(!app_kinds(&d).contains(&App::Jumbf));
}

#[test]
fn all_fixtures_end_with_eoi() {
    for name in common::ALL {
        let d = common::fixture(name);
        assert_eq!(check_eoi(&d), Ok(()), "{name} は EOI で終わる");
    }
}

#[test]
fn truncated_scans_but_fails_eoi_check() {
    let d = common::fixture("truncated.jpg");
    // SOS 到達で走査を打ち切るので、スキャンデータが切れていても走査自体は成功する
    assert!(scan(&d).is_ok(), "走査は成功する");
    // 切り詰めは EOI の検証で捕まえる
    assert_eq!(check_eoi(&d), Err(JpegError::MissingEoi));
}

#[test]
fn non_jpeg_is_rejected() {
    assert_eq!(
        scan(b"\x89PNG\r\n\x1a\n").unwrap_err(),
        JpegError::MissingSoi
    );
    assert_eq!(scan(b"").unwrap_err(), JpegError::MissingSoi);
    assert_eq!(scan(b"\xff\xd8").unwrap_err(), JpegError::MissingSoi);
}

#[test]
fn segment_with_bad_length_is_rejected() {
    // APP1 の長さフィールドが 0（最小は自身の 2 バイト）
    let d = b"\xff\xd8\xff\xe1\x00\x00\xff\xda\x00\x00\xff\xd9";
    assert!(matches!(
        scan(d).unwrap_err(),
        JpegError::BadLength { marker: 0xE1, .. }
    ));
}

#[test]
fn segment_running_past_eof_is_rejected() {
    // APP1 が 1000 バイトあると宣言しているがデータが足りない
    let d = b"\xff\xd8\xff\xe1\x03\xe8ab";
    assert!(matches!(
        scan(d).unwrap_err(),
        JpegError::TruncatedSegment { marker: 0xE1, .. }
    ));
}

#[test]
fn missing_sos_is_rejected() {
    // APP1 だけで SOS が来ないまま終わる
    let d = b"\xff\xd8\xff\xe1\x00\x04ab";
    assert_eq!(scan(d).unwrap_err(), JpegError::MissingSos);
}

#[test]
fn scan_data_is_the_pixel_payload() {
    let d = common::fixture("lrc_firefly.jpg");
    let segs = scan(&d).unwrap();
    let sd = scan_data(&d, &segs);
    assert!(sd.starts_with(b"\xff\xda"));
    assert!(sd.ends_with(b"\xff\xd9"));
    // 画素は全体の一部でしかないが、末尾までを占める
    assert_eq!(
        sd.len(),
        d.len() - segs.iter().find(|s| s.marker == SOS).unwrap().start
    );
}

#[test]
fn restart_markers_inside_scan_are_not_parsed() {
    // SOS 以降に FF D0（RST0）と FF 00（バイトスタッフィング）を置いても
    // 追加のセグメントとして解釈されないこと
    let d = b"\xff\xd8\xff\xda\x00\x02\xff\x00\xff\xd0\xff\xe1\x00\x04ab\xff\xd9";
    let segs = scan(d).unwrap();
    assert_eq!(segs.len(), 1, "SOS 1 つだけ: {segs:?}");
    assert_eq!(segs[0].marker, SOS);
    assert_eq!(segs[0].end, d.len());
}
