//! 診断。どのレイヤーに何が入っていて、なぜ AI 判定されるのかを説明する。

use crate::c2pa;
use crate::jpeg::segment::{scan, App, JpegError, Segment};
use crate::jumbf;
use crate::xmp;

/// EXIF 内のプライバシー情報。v0.1 では除去せず警告だけ出す（設計書 §4.2）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExifPrivacy {
    pub gps: bool,
    pub body_serial: bool,
    pub lens_serial: bool,
    pub owner_name: bool,
    pub maker_note: bool,
}

impl ExifPrivacy {
    pub fn any(&self) -> bool {
        self.gps || self.body_serial || self.lens_serial || self.owner_name || self.maker_note
    }

    /// 見つかった項目の名前。
    pub fn found(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.gps {
            v.push("GPS 座標");
        }
        if self.body_serial {
            v.push("ボディシリアル");
        }
        if self.lens_serial {
            v.push("レンズシリアル");
        }
        if self.owner_name {
            v.push("所有者名");
        }
        if self.maker_note {
            v.push("MakerNote");
        }
        v
    }
}

/// 1 セグメントの所見。
#[derive(Debug, Clone)]
pub struct SegmentInfo {
    pub name: String,
    pub bytes: usize,
    pub note: String,
}

/// 診断結果。
#[derive(Debug, Default)]
pub struct Inspection {
    pub width: u16,
    pub height: u16,
    pub bytes: usize,
    pub segments: Vec<SegmentInfo>,
    pub actions: Option<c2pa::Actions>,
    pub claim: Option<c2pa::Claim>,
    pub signer: Option<c2pa::Signer>,
    pub manifest_label: Option<String>,
    /// XMP に残っている除去対象プロパティ
    pub xmp_leaks: Vec<String>,
    /// XMP に Firefly 由来の痕跡があるか
    pub firefly: bool,
    pub exif_privacy: ExifPrivacy,
}

impl Inspection {
    /// X が「AIで作成」を付ける根拠があるか。
    pub fn ai_flagged(&self) -> bool {
        self.actions.as_ref().is_some_and(|a| a.declares_ai())
    }

    /// C2PA マニフェストがあるか。
    pub fn has_c2pa(&self) -> bool {
        self.manifest_label.is_some()
    }
}

/// EXIF の IFD を走査して、プライバシーに関わるタグの有無だけを調べる。
///
/// 値は読まない。除去は v0.2 で行うため、ここでは存在を検出して警告するだけ。
fn exif_privacy(payload: &[u8]) -> ExifPrivacy {
    const GPS_IFD: u16 = 0x8825;
    const EXIF_IFD: u16 = 0x8769;
    const MAKER_NOTE: u16 = 0x927C;
    const OWNER_NAME: u16 = 0xA430;
    const BODY_SERIAL: u16 = 0xA431;
    const LENS_SERIAL: u16 = 0xA435;

    let mut out = ExifPrivacy::default();

    // "Exif\0\0" の後ろが TIFF ヘッダ
    let Some(tiff) = payload.get(6..) else {
        return out;
    };
    if tiff.len() < 8 {
        return out;
    }
    let le = match &tiff[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return out,
    };
    let u16at = |b: &[u8]| -> u16 {
        if le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        }
    };
    let u32at = |b: &[u8]| -> u32 {
        if le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        }
    };

    // IFD を辿る。入れ子は Exif IFD の 1 段だけ見れば足りる
    let mut queue = vec![u32at(&tiff[4..8]) as usize];
    let mut visited = 0;

    while let Some(off) = queue.pop() {
        visited += 1;
        if visited > 8 || off + 2 > tiff.len() {
            break;
        }
        let count = u16at(&tiff[off..off + 2]) as usize;
        for n in 0..count {
            let e = off + 2 + n * 12;
            if e + 12 > tiff.len() {
                break;
            }
            let tag = u16at(&tiff[e..e + 2]);
            match tag {
                GPS_IFD => out.gps = true,
                MAKER_NOTE => out.maker_note = true,
                OWNER_NAME => out.owner_name = true,
                BODY_SERIAL => out.body_serial = true,
                LENS_SERIAL => out.lens_serial = true,
                EXIF_IFD => queue.push(u32at(&tiff[e + 8..e + 12]) as usize),
                _ => {}
            }
        }
    }

    out
}

/// SOF セグメントから画像サイズを読む。
fn dimensions(data: &[u8], segs: &[Segment]) -> (u16, u16) {
    for s in segs {
        // SOF0/1/2/3, SOF5-7, SOF9-11, SOF13-15
        if matches!(s.marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            let p = s.payload_bytes(data);
            if p.len() >= 5 {
                return (
                    u16::from_be_bytes([p[3], p[4]]),
                    u16::from_be_bytes([p[1], p[2]]),
                );
            }
        }
    }
    (0, 0)
}

/// ファイルを診断する。ファイルは読むだけで書き換えない。
pub fn inspect(data: &[u8]) -> Result<Inspection, JpegError> {
    let segs = scan(data)?;
    let (w, h) = dimensions(data, &segs);

    let mut out = Inspection {
        width: w,
        height: h,
        bytes: data.len(),
        ..Default::default()
    };

    // 分割されたマニフェストを Packet sequence 順に連結する
    let mut app11: Vec<(u32, &[u8])> = Vec::new();

    for seg in &segs {
        let Some(kind) = seg.app_kind(data) else {
            continue;
        };
        let payload = seg.payload_bytes(data);

        let (name, note) = match kind {
            App::Jumbf => {
                if let (Some(z), Some(body)) = (
                    jumbf::packet_sequence(payload),
                    jumbf::strip_app11_header(payload),
                ) {
                    app11.push((z, body));
                }
                ("APP11/JUMBF", "C2PA マニフェスト".to_string())
            }
            App::Exif => {
                out.exif_privacy = exif_privacy(payload);
                let found = out.exif_privacy.found();
                let note = if found.is_empty() {
                    "撮影データ・著作権".to_string()
                } else {
                    format!("撮影データ・著作権（{} を含む）", found.join(" / "))
                };
                ("APP1/Exif", note)
            }
            App::Xmp => {
                let xml = &payload[xmp::XMP_HEADER.len().min(payload.len())..];
                // 診断では何も除外せず、残っている追跡情報を全部拾う
                if let Some((_, outcome)) = xmp::strip(xml, &xmp::Keep::default()) {
                    out.xmp_leaks = outcome.removed.iter().map(|r| r.name.clone()).collect();
                }
                out.firefly = xml.windows(7).any(|w| w == b"firefly");
                let note = if out.xmp_leaks.is_empty() {
                    "現像設定・著作権".to_string()
                } else {
                    out.xmp_leaks.join(", ")
                };
                ("APP1/XMP", note)
            }
            App::XmpExtension => ("APP1/XMP(拡張)", "編集できないため保持する".to_string()),
            App::Icc => ("APP2/ICC", "カラープロファイル".to_string()),
            App::Mpf => (
                "APP2/MPF",
                "絶対オフセットを含む（先行削除時は道連れ）".to_string(),
            ),
            App::Photoshop => ("APP13/IPTC", "IPTC・Photoshop リソース".to_string()),
            App::Adobe => ("APP14/Adobe", "ColorTransform 宣言".to_string()),
            App::Jfif => ("APP0/JFIF", "解像度".to_string()),
            App::Unknown(n) => {
                out.segments.push(SegmentInfo {
                    name: format!("APP{n}"),
                    bytes: seg.size(),
                    note: "未知のベンダー拡張".to_string(),
                });
                continue;
            }
        };

        out.segments.push(SegmentInfo {
            name: name.to_string(),
            bytes: seg.size(),
            note,
        });
    }

    if !app11.is_empty() {
        app11.sort_by_key(|(z, _)| *z);
        let joined: Vec<u8> = app11.iter().flat_map(|(_, b)| b.iter().copied()).collect();
        if let Some(m) = jumbf::read_manifest(&joined) {
            out.manifest_label = m.label.clone();
            // ストア内のどのマニフェストが AI を宣言していても拾う
            let mut merged = c2pa::Actions::default();
            let mut found = false;
            for label in ["c2pa.actions.v2", "c2pa.actions"] {
                for cbor in m.assertions_named(label) {
                    if let Some(a) = c2pa::read_actions(cbor) {
                        found = true;
                        merged.app_enforced |= a.app_enforced;
                        merged.actions.extend(a.actions);
                    }
                }
            }
            if found {
                out.actions = Some(merged);
            }
            if let Some(cbor) = m.claim {
                out.claim = c2pa::read_claim(cbor);
            }
            if let Some(cbor) = m.signature {
                out.signer = c2pa::read_signer(cbor);
            }
        }
    }

    Ok(out)
}
