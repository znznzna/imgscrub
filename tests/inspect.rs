//! 診断のテスト。期待値は実ファイルの実測に基づく（設計書 §1.1）。

mod common;

use imgscrub::inspect::inspect;
use imgscrub::jpeg::filter::KeepSet;
use imgscrub::jumbf;

#[test]
fn firefly_file_is_flagged_with_the_reason() {
    let d = common::fixture("lrc_firefly.jpg");
    let i = inspect(&d).unwrap();

    assert!(i.has_c2pa());
    assert!(i.ai_flagged(), "AI 判定の根拠が検出される");
    assert_eq!(
        i.manifest_label.as_deref(),
        Some("urn:c2pa:e7a6bef0-e990-48eb-91a9-220bec674373:adobe")
    );

    let a = i.actions.expect("actions.v2 が読める");
    assert!(a.app_enforced, "強制付与フラグを読めている");

    // 実測では c2pa.opened / drawing / edited の 3 つ
    let names: Vec<&str> = a.actions.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, vec!["c2pa.opened", "c2pa.drawing", "c2pa.edited"]);

    let ai: Vec<&imgscrub::c2pa::Action> = a.actions.iter().filter(|x| x.declares_ai()).collect();
    assert_eq!(ai.len(), 2, "drawing と edited が AI を宣言する");
    assert_eq!(
        ai[0].software_agent.as_deref(),
        Some("Adobe Remove Object 1")
    );
    assert!(ai[0]
        .digital_source_type
        .as_deref()
        .unwrap()
        .ends_with("compositeWithTrainedAlgorithmicMedia"));
    assert!(ai[0]
        .params
        .iter()
        .any(|(k, v)| k == "com.adobe.acr.value" && v.contains("Uses GenAI")));
    assert!(ai[0]
        .params
        .iter()
        .any(|(k, v)| k == "com.adobe.firefly.version" && v.starts_with("clio-erase")));
}

#[test]
fn claim_generator_is_read() {
    let d = common::fixture("lrc_firefly.jpg");
    let c = inspect(&d).unwrap().claim.expect("claim.v2 が読める");
    assert_eq!(c.generator_name.as_deref(), Some("Adobe Lightroom Classic"));
    assert_eq!(c.generator_version.as_deref(), Some("15.5.1"));
    assert_eq!(c.spec_version.as_deref(), Some("2.4.0"));
}

/// 署名者と発行者は証明書の CN から分離して読める。
///
/// 発行者が一時 CA なので、検証サイトでは「クレデンシャルなし」になるが
/// X はアサーションを読む——という非対称の説明に必要な情報。
#[test]
fn signer_and_issuer_are_separated() {
    let d = common::fixture("lrc_firefly.jpg");
    let s = inspect(&d).unwrap().signer.expect("署名が読める");
    assert_eq!(s.subject_cn.as_deref(), Some("Adobe Compliance Signer"));
    assert_eq!(s.issuer_cn.as_deref(), Some("c2pa-ephemeral-ca.local"));
}

#[test]
fn clean_file_is_not_flagged() {
    let d = common::fixture("lrc_clean.jpg");
    let i = inspect(&d).unwrap();
    assert!(!i.has_c2pa());
    assert!(!i.ai_flagged());
    assert!(!i.firefly);
    // C2PA がなくても追跡 ID は残っている
    assert!(i.xmp_leaks.iter().any(|l| l == "xmpMM:PreservedFileName"));
}

#[test]
fn dimensions_are_read_from_sof() {
    // フィクスチャは画素だけ 16x16 に縮小してある
    let i = inspect(&common::fixture("lrc_firefly.jpg")).unwrap();
    assert_eq!((i.width, i.height), (16, 16));
}

#[test]
fn segment_inventory_matches_measurement() {
    let i = inspect(&common::fixture("lrc_firefly.jpg")).unwrap();
    let names: Vec<&str> = i.segments.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "APP11/JUMBF",
            "APP1/Exif",
            "APP13/IPTC",
            "APP2/ICC",
            "APP1/XMP",
            "APP14/Adobe"
        ]
    );
    let jumbf = &i.segments[0];
    assert_eq!(jumbf.bytes, 14_478);
}

#[test]
fn inspect_never_touches_the_file() {
    for name in common::ALL {
        let d = common::fixture(name);
        let before = d.clone();
        let _ = inspect(&d);
        assert_eq!(before, d, "{name}: 入力バッファが変わった");
    }
}

#[test]
fn broken_file_reports_an_error() {
    assert!(inspect(b"not a jpeg").is_err());
}

#[test]
fn jumbf_boxes_are_walked() {
    let d = common::fixture("lrc_firefly.jpg");
    let segs = imgscrub::jpeg::segment::scan(&d).unwrap();
    let app11 = segs
        .iter()
        .find(|s| s.app_kind(&d) == Some(imgscrub::jpeg::segment::App::Jumbf))
        .unwrap();
    let payload = app11.payload_bytes(&d);

    assert_eq!(jumbf::packet_sequence(payload), Some(1));
    let body = jumbf::strip_app11_header(payload).unwrap();
    let m = jumbf::read_manifest(body).unwrap();

    let labels: Vec<&str> = m.assertions.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(
        labels,
        vec!["c2pa.ingredient.v3", "c2pa.actions.v2", "c2pa.hash.data"]
    );
    assert!(m.claim.is_some());
    assert!(m.signature.is_some());
    assert!(m.assertion("c2pa.actions.v2").is_some());
    assert!(m.assertion("存在しない").is_none());
}

/// 壊れた箱で無限ループやパニックを起こさない。
#[test]
fn malformed_jumbf_is_survivable() {
    assert!(jumbf::parse(&[]).is_empty());
    assert!(jumbf::parse(&[0, 0, 0, 0]).is_empty());
    // LBox が中身より大きいと申告している
    assert!(jumbf::parse(&[0xFF, 0xFF, 0xFF, 0xFF, b'j', b'u', b'm', b'b']).is_empty());
    // LBox が 0（末尾まで）
    let boxes = jumbf::parse(&[0, 0, 0, 0, b'c', b'b', b'o', b'r', 1, 2, 3]);
    assert_eq!(boxes.len(), 1);
    assert_eq!(boxes[0].content, &[1, 2, 3]);
    assert!(jumbf::description_label(&[]).is_none());
}

#[test]
fn keepset_parses_and_rejects_typos() {
    let k = KeepSet::parse(&["xmpmm", "crs"]).unwrap();
    assert!(k.xmpmm && k.crs && !k.mpf && !k.unknown);
    assert!(KeepSet::parse(&["XMPMM"]).unwrap().xmpmm, "大文字も受ける");
    assert_eq!(KeepSet::parse(&["xmpmmm"]).unwrap_err(), "xmpmmm");
    assert_eq!(KeepSet::parse::<&str>(&[]).unwrap(), KeepSet::default());
}
