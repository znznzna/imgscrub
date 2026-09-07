//! XMP のプロパティ単位除去のテスト。

mod common;

use imgscrub::jpeg::filter::Options;
use imgscrub::jpeg::segment::{scan, App};
use imgscrub::scrub::process;
use imgscrub::xmp;

/// 何も除外しない（全部除去する）設定。
const K: xmp::Keep = xmp::Keep {
    xmpmm: false,
    crs: false,
};

/// 出力から XMP の XML を取り出す。
fn xmp_of(data: &[u8]) -> Option<String> {
    let segs = scan(data).ok()?;
    let seg = segs.iter().find(|s| s.app_kind(data) == Some(App::Xmp))?;
    let p = seg.payload_bytes(data);
    Some(String::from_utf8_lossy(&p[xmp::XMP_HEADER.len()..]).into_owned())
}

#[test]
fn firefly_traces_are_removed() {
    let d = common::fixture("lrc_firefly.jpg");
    let (out, _) = process(&d, &Options::default()).unwrap();
    let x = xmp_of(&out).expect("XMP は残る");

    for needle in [
        "RemoveAreas",
        "fill_method",
        "firefly",
        "PreservedFileName",
        "DocumentID",
        "InstanceID",
        "History",
        "DerivedFrom",
    ] {
        assert!(!x.contains(needle), "XMP に {needle} が残っている");
    }
}

#[test]
fn copyright_and_lens_survive() {
    let d = common::fixture("lrc_firefly.jpg");
    let (out, _) = process(&d, &Options::default()).unwrap();
    let x = xmp_of(&out).unwrap();

    assert!(x.contains("Motoki Endo"), "著作権表記が消えた");
    assert!(x.contains("APO-LANTHAR"), "レンズ情報が消えた");
    assert!(x.contains("crs:"), "現像設定の他のプロパティは残る");
}

#[test]
fn output_xmp_is_well_formed() {
    for name in ["lrc_firefly.jpg", "lrc_clean.jpg"] {
        let d = common::fixture(name);
        let (out, _) = process(&d, &Options::default()).unwrap();
        let x = xmp_of(&out).unwrap();
        // xpacket ラッパーと足場が保たれている
        assert!(x.contains("<?xpacket begin="), "{name}: xpacket 開始がない");
        assert!(x.contains("<?xpacket end="), "{name}: xpacket 終了がない");
        assert!(x.contains("</x:xmpmeta>"), "{name}: xmpmeta が閉じていない");
        // strip は EOF 時点の深さを検証するので、通ればタグの対応が取れている。
        // 2 回目は何も除去されないこと（冪等）も同時に確認できる。
        let (_, again) = xmp::strip(x.as_bytes(), &xmp::Keep::default())
            .unwrap_or_else(|| panic!("{name}: 出力 XMP を再パースできない"));
        assert!(
            again.removed.is_empty(),
            "{name}: 2 回目に除去対象が残っていた {:?}",
            again.removed
        );
    }
}

#[test]
fn c2pa_only_leaves_xmp_untouched() {
    let d = common::fixture("lrc_firefly.jpg");
    let before = xmp_of(&d).unwrap();
    let (out, _) = process(
        &d,
        &Options {
            c2pa_only: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        before,
        xmp_of(&out).unwrap(),
        "--c2pa-only で XMP が変わった"
    );
    assert!(before.contains("firefly"), "元には firefly がある");
}

#[test]
fn clean_export_still_loses_tracking_ids() {
    // 生成AI 未使用でも追跡用 UUID と元ファイル名は残っているので除去される
    let d = common::fixture("lrc_clean.jpg");
    assert!(xmp_of(&d).unwrap().contains("PreservedFileName"));

    let (out, report) = process(&d, &Options::default()).unwrap();
    assert!(report.changed(), "C2PA がなくても XMP の除去は起きる");
    let x = xmp_of(&out).unwrap();
    assert!(!x.contains("PreservedFileName"));
    assert!(x.contains("Motoki Endo"), "著作権は残る");
}

/// xpacket のパディングは `</x:xmpmeta>` と `<?xpacket end?>` の間にある空白。
fn padding_len(xml: &str) -> usize {
    match xml.split_once("</x:xmpmeta>") {
        Some((_, tail)) => tail.len() - tail.trim_start().len(),
        None => 0,
    }
}

#[test]
fn xpacket_padding_is_dropped() {
    let d = common::fixture("lrc_firefly.jpg");
    let before = xmp_of(&d).unwrap();
    // 実測で 4KB 超の空白パディングが入っている
    let pad_before = padding_len(&before);
    assert!(pad_before > 1000, "元にパディングがある前提: {pad_before}");

    let (out, _) = process(&d, &Options::default()).unwrap();
    let after = xmp_of(&out).unwrap();
    assert!(
        padding_len(&after) < 100,
        "パディングが残っている: {}",
        padding_len(&after)
    );
    // パディングを落とした分だけでも XMP は明確に縮む
    assert!(after.len() + pad_before <= before.len() + 100);
}

#[test]
fn broken_xml_is_kept_unedited() {
    assert!(xmp::strip(b"<x:xmpmeta><unclosed>", &K).is_none());
    assert!(xmp::strip(b"\xff\xfe not utf-8 \xff", &K).is_none());
}

#[test]
fn scaffolding_only_xmp_is_reported_empty() {
    let xml = concat!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>"#,
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">"#,
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">"#,
        r#"<rdf:Description rdf:about="" xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/""#,
        r#" xmpMM:DocumentID="xmp.did:deadbeef"/>"#,
        r#"</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#,
    );
    let (_, outcome) = xmp::strip(xml.as_bytes(), &K).unwrap();
    assert_eq!(outcome.removed.len(), 1);
    assert!(outcome.is_empty, "追跡 ID しかない XMP は空になる");
}

#[test]
fn unrelated_properties_are_untouched() {
    let xml = concat!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">"#,
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">"#,
        r#"<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/""#,
        r#" dc:format="image/jpeg"/>"#,
        r#"</rdf:RDF></x:xmpmeta>"#,
    );
    let (out, outcome) = xmp::strip(xml.as_bytes(), &K).unwrap();
    assert!(outcome.removed.is_empty());
    assert!(!outcome.is_empty);
    assert!(String::from_utf8_lossy(&out).contains("image/jpeg"));
}

/// 名前空間の接頭辞が非標準でも URI で照合できる。
#[test]
fn matching_is_by_namespace_not_prefix() {
    let xml = concat!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">"#,
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">"#,
        r#"<rdf:Description rdf:about="" xmlns:weird="http://ns.adobe.com/xap/1.0/mm/""#,
        r#" xmlns:dc="http://purl.org/dc/elements/1.1/""#,
        r#" weird:PreservedFileName="secret.ARQ" dc:format="image/jpeg"/>"#,
        r#"</rdf:RDF></x:xmpmeta>"#,
    );
    let (out, outcome) = xmp::strip(xml.as_bytes(), &K).unwrap();
    assert_eq!(outcome.removed.len(), 1, "接頭辞が weird でも除去される");
    let s = String::from_utf8_lossy(&out);
    assert!(!s.contains("secret.ARQ"));
    assert!(s.contains("image/jpeg"));
}
