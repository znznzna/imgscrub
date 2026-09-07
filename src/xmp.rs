//! XMP のプロパティ単位の除去。
//!
//! 正規表現は使わない。XMP のプロパティは要素と属性のどちらでも表現でき、名前空間の
//! 接頭辞も文書ごとに変わりうるため、名前空間 URI とローカル名の組で照合する。
//!
//! 実測（Lightroom Classic 15.5.1 の書き出し）での表現形式:
//!
//! | プロパティ | 形式 |
//! |---|---|
//! | `crs:RemoveAreas` | 要素（`rdf:Seq` を子に持つ） |
//! | `crs:fill_method` | 属性（`RemoveAreas` の内側） |
//! | `xmpMM:DocumentID` / `InstanceID` / `OriginalDocumentID` / `PreservedFileName` | 属性 |
//! | `xmpMM:History` / `DerivedFrom` | 要素 |

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{Namespace, ResolveResult};
use quick_xml::{NsReader, Writer};

/// APP1/XMP セグメントのペイロード先頭に付く識別子。
pub const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

const NS_CRS: Namespace = Namespace(b"http://ns.adobe.com/camera-raw-settings/1.0/");
const NS_XMPMM: Namespace = Namespace(b"http://ns.adobe.com/xap/1.0/mm/");
const NS_DCTERMS: Namespace = Namespace(b"http://purl.org/dc/terms/");
const NS_RDF: Namespace = Namespace(b"http://www.w3.org/1999/02/22-rdf-syntax-ns#");
const NS_XMPMETA: Namespace = Namespace(b"adobe:ns:meta/");

/// 除去した 1 プロパティ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    /// `crs:RemoveAreas` のような接頭辞付きの名前
    pub name: String,
    /// なぜ消すのか
    pub why: &'static str,
}

/// 除去の結果。
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub removed: Vec<Removed>,
    /// 意味のあるプロパティが残っていないか。呼び出し側はセグメントごと落とせる
    pub is_empty: bool,
}

/// 除去対象なら理由を返す。要素と属性で共通。
fn removal_reason(ns: &ResolveResult, local: &[u8]) -> Option<&'static str> {
    let ns = match ns {
        ResolveResult::Bound(n) => *n,
        _ => return None,
    };

    if ns == NS_CRS {
        return match local {
            b"RemoveAreas" => Some("生成AI消しゴムの適用履歴"),
            b"fill_method" => Some("塗りつぶし方式（firefly か否かが露出する）"),
            _ => None,
        };
    }

    if ns == NS_XMPMM {
        return match local {
            b"PreservedFileName" => Some("元ファイル名"),
            b"DocumentID" | b"InstanceID" | b"OriginalDocumentID" => {
                Some("原本を横断追跡できる UUID")
            }
            b"History" => Some("編集履歴"),
            b"DerivedFrom" => Some("派生元の参照"),
            _ => None,
        };
    }

    if ns == NS_DCTERMS && local == b"provenance" {
        return Some("C2PA クラウドマニフェストへの参照");
    }

    None
}

/// 足場となる要素。プロパティが残っているかの判定では数えない。
fn is_scaffolding(ns: &ResolveResult, local: &[u8]) -> bool {
    match ns {
        ResolveResult::Bound(n) if *n == NS_XMPMETA => local == b"xmpmeta",
        ResolveResult::Bound(n) if *n == NS_RDF => local == b"RDF" || local == b"Description",
        _ => false,
    }
}

fn is_ns_decl(key: &[u8]) -> bool {
    key == b"xmlns" || key.starts_with(b"xmlns:")
}

/// 除去対象の属性を落とした要素を作り、残った実プロパティ属性の数を返す。
fn rebuild<'a>(
    e: &BytesStart<'a>,
    reader: &NsReader<&[u8]>,
    outcome: &mut Outcome,
) -> Option<(BytesStart<'a>, usize)> {
    let name = e.name();
    let mut out = BytesStart::from_content(
        String::from_utf8_lossy(name.as_ref()).into_owned(),
        name.as_ref().len(),
    );
    let mut kept = 0usize;

    for attr in e.attributes().with_checks(false) {
        let attr = attr.ok()?;
        let key = attr.key;

        if is_ns_decl(key.as_ref()) {
            out.push_attribute(attr);
            continue;
        }

        let (ns, local) = reader.resolve_attribute(key);
        if let Some(why) = removal_reason(&ns, local.as_ref()) {
            outcome.removed.push(Removed {
                name: String::from_utf8_lossy(key.as_ref()).into_owned(),
                why,
            });
            continue;
        }

        // rdf:about は足場なのでプロパティとして数えない
        if key.as_ref() != b"rdf:about" {
            kept += 1;
        }
        out.push_attribute(attr);
    }

    Some((out, kept))
}

/// XMP の XML からプロパティを除去する。
///
/// `None` を返すのは、UTF-8 でない / XML として壊れている / 再直列化で膨らんだ場合。
/// 呼び出し側は XMP を無編集で保持する。
pub fn strip(xml_bytes: &[u8]) -> Option<(Vec<u8>, Outcome)> {
    let xml = std::str::from_utf8(xml_bytes).ok()?;

    let mut reader = NsReader::from_str(xml);
    reader.config_mut().trim_text(false);
    reader.config_mut().check_end_names = true;

    let mut writer = Writer::new(Vec::new());
    let mut outcome = Outcome::default();
    let mut props = 0usize;
    let mut depth = 0i32;

    loop {
        let (ns, ev) = reader.read_resolved_event().ok()?;

        match ev {
            Event::Eof => break,

            Event::Start(e) => {
                let local = e.local_name();
                if let Some(why) = removal_reason(&ns, local.as_ref()) {
                    outcome.removed.push(Removed {
                        name: String::from_utf8_lossy(e.name().as_ref()).into_owned(),
                        why,
                    });
                    reader.read_to_end(e.name()).ok()?; // 子孫ごと捨てる
                    continue;
                }
                if !is_scaffolding(&ns, local.as_ref()) {
                    props += 1;
                }
                let (rebuilt, kept) = rebuild(&e, &reader, &mut outcome)?;
                props += kept;
                depth += 1;
                writer.write_event(Event::Start(rebuilt)).ok()?;
            }

            Event::Empty(e) => {
                let local = e.local_name();
                if let Some(why) = removal_reason(&ns, local.as_ref()) {
                    outcome.removed.push(Removed {
                        name: String::from_utf8_lossy(e.name().as_ref()).into_owned(),
                        why,
                    });
                    continue;
                }
                if !is_scaffolding(&ns, local.as_ref()) {
                    props += 1;
                }
                let (rebuilt, kept) = rebuild(&e, &reader, &mut outcome)?;
                props += kept;
                writer.write_event(Event::Empty(rebuilt)).ok()?;
            }

            Event::End(e) => {
                depth -= 1;
                writer.write_event(Event::End(e)).ok()?;
            }

            // ルート要素の外側にある空白は xpacket のパディング。他ツールが in-place 編集
            // するためだけのもので、imgscrub は常に全体を書き直すため落とす（実測で約 4KB）。
            Event::Text(t) if depth == 0 && t.iter().all(|b| b.is_ascii_whitespace()) => {}

            other => {
                writer.write_event(other).ok()?;
            }
        }
    }

    // quick-xml の check_end_names は閉じられないまま EOF に達したタグを検出しない。
    // 深さが残っていたら XML が壊れているので編集を諦める（黙って別の XML にしない）。
    if depth != 0 {
        return None;
    }

    let out = writer.into_inner();
    if out.len() > xml_bytes.len() {
        return None;
    }

    outcome.is_empty = props == 0;
    Some((out, outcome))
}
