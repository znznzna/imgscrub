//! JUMBF（ISO/IEC 19566-5）の箱構造の読み取り。
//!
//! APP11 のペイロードは共通識別子 `JP` + Box Instance(2) + Packet sequence(4) の 8 バイト
//! に続いて JUMBF スーパーボックスが並ぶ。マニフェストが 64KB を超える場合は複数の APP11
//! に分割され、Packet sequence 番号の順に連結して 1 つの箱列になる。
//!
//! 実測（Lightroom Classic 15.5.1）での階層:
//!
//! ```text
//! jumb "c2pa"
//!   jumb "urn:c2pa:<uuid>:adobe"
//!     jumb "c2pa.assertions"
//!       jumb "c2pa.ingredient.v3" → cbor
//!       jumb "c2pa.actions.v2"    → cbor   ← X が読む実体
//!       jumb "c2pa.hash.data"     → cbor
//!     jumb "c2pa.claim.v2"        → cbor
//!     jumb "c2pa.signature"       → cbor（COSE_Sign1）
//! ```

/// APP11 ペイロードの先頭に付く共通識別子。
const CI: &[u8] = b"JP";
/// CI(2) + Box Instance(2) + Packet sequence(4)
const APP11_HEADER: usize = 8;

/// 1 つの JUMBF 箱。
#[derive(Debug, Clone)]
pub struct Chunk<'a> {
    pub typ: [u8; 4],
    pub content: &'a [u8],
}

/// APP11 のペイロードが JUMBF か判定し、箱列の先頭を返す。
pub fn strip_app11_header(payload: &[u8]) -> Option<&[u8]> {
    if payload.len() > APP11_HEADER && payload.starts_with(CI) {
        Some(&payload[APP11_HEADER..])
    } else {
        None
    }
}

/// APP11 のペイロードから Packet sequence 番号を読む。分割マニフェストの並べ替えに使う。
pub fn packet_sequence(payload: &[u8]) -> Option<u32> {
    if payload.len() < APP11_HEADER || !payload.starts_with(CI) {
        return None;
    }
    Some(u32::from_be_bytes([
        payload[4], payload[5], payload[6], payload[7],
    ]))
}

/// 同じ階層に並ぶ箱を列挙する。
pub fn parse(data: &[u8]) -> Vec<Chunk<'_>> {
    let mut out = Vec::new();
    let mut i = 0usize;

    while i + 8 <= data.len() {
        let lbox = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        let mut typ = [0u8; 4];
        typ.copy_from_slice(&data[i + 4..i + 8]);

        // LBox が 1 なら XLBox(8) が続く。0 なら末尾まで
        let (size, hdr) = match lbox {
            1 => {
                if i + 16 > data.len() {
                    break;
                }
                let x = u64::from_be_bytes(data[i + 8..i + 16].try_into().unwrap());
                (x as usize, 16usize)
            }
            0 => (data.len() - i, 8usize),
            n => (n as usize, 8usize),
        };

        if size < hdr || i + size > data.len() {
            break; // 壊れているので打ち切る
        }

        out.push(Chunk {
            typ,
            content: &data[i + hdr..i + size],
        });
        i += size;
    }

    out
}

/// 記述箱（`jumd`）から自身のラベルを読む。
///
/// レイアウト: type UUID(16) + toggles(1) + [ラベル（NUL 終端）if toggles & 0x02]。
/// 以降にも項目が続きうるが、ラベルより後ろは読まない。
pub fn description_label(jumd: &[u8]) -> Option<String> {
    if jumd.len() < 17 {
        return None;
    }
    let toggles = jumd[16];
    if toggles & 0x02 == 0 {
        return None;
    }
    let rest = &jumd[17..];
    let end = rest.iter().position(|b| *b == 0)?;
    String::from_utf8(rest[..end].to_vec()).ok()
}

/// ラベル付きのスーパーボックス。
#[derive(Debug, Clone)]
pub struct Labeled<'a> {
    pub label: String,
    /// この箱の中身（`jumd` を含む）
    pub content: &'a [u8],
}

/// スーパーボックス（`jumb`）の直下の子を、ラベル付きで列挙する。
pub fn children(superbox_content: &[u8]) -> Vec<Labeled<'_>> {
    let mut out = Vec::new();
    for chunk in parse(superbox_content) {
        if &chunk.typ != b"jumb" {
            continue;
        }
        let inner = parse(chunk.content);
        let label = inner
            .iter()
            .find(|c| &c.typ == b"jumd")
            .and_then(|c| description_label(c.content));
        if let Some(label) = label {
            out.push(Labeled {
                label,
                content: chunk.content,
            });
        }
    }
    out
}

/// スーパーボックスの中の指定した型の中身を返す（`cbor` など）。
pub fn payload_of_type<'a>(superbox_content: &'a [u8], typ: &[u8; 4]) -> Option<&'a [u8]> {
    parse(superbox_content)
        .into_iter()
        .find(|c| &c.typ == typ)
        .map(|c| c.content)
}

/// マニフェストストアから読み取った内容。
///
/// ストアには複数のマニフェストが入りうる（取り込んだ素材が持ち込んだもの）。
/// C2PA 仕様ではストアの**最後**のマニフェストが active manifest になる。
/// 診断としては「どこかで AI が宣言されていれば拾いたい」ので、アサーションは
/// 全マニフェストから集め、生成元と署名は active manifest のものを採る。
#[derive(Debug, Default)]
pub struct Manifest<'a> {
    /// active manifest のラベル `urn:c2pa:<uuid>:<generator>`
    pub label: Option<String>,
    /// 全マニフェストのアサーション（ラベルと CBOR）。ラベルは重複しうる
    pub assertions: Vec<(String, &'a [u8])>,
    /// active manifest の claim
    pub claim: Option<&'a [u8]>,
    /// active manifest の署名
    pub signature: Option<&'a [u8]>,
    /// ストアに入っていたマニフェストの数
    pub manifest_count: usize,
}

impl<'a> Manifest<'a> {
    /// ラベルでアサーションの CBOR を引く（最初の 1 件）。
    pub fn assertion(&self, label: &str) -> Option<&'a [u8]> {
        self.assertions_named(label).into_iter().next()
    }

    /// 同じラベルのアサーションを全部返す。マニフェストが複数ある場合に効く。
    pub fn assertions_named(&self, label: &str) -> Vec<&'a [u8]> {
        self.assertions
            .iter()
            .filter(|(l, _)| l == label)
            .map(|(_, c)| *c)
            .collect()
    }
}

/// JUMBF の箱列から C2PA マニフェストストアを読む。
pub fn read_manifest(boxes: &[u8]) -> Option<Manifest<'_>> {
    // 最外の jumb "c2pa" がマニフェストストア
    let store = parse(boxes).into_iter().find(|c| &c.typ == b"jumb")?;

    let mut m = Manifest::default();

    for manifest in children(store.content) {
        m.manifest_count += 1;
        // 後のマニフェストで上書きしていくので、最後に残るのが active manifest
        m.label = Some(manifest.label.clone());

        for part in children(manifest.content) {
            match part.label.as_str() {
                "c2pa.assertions" => {
                    for a in children(part.content) {
                        if let Some(cbor) = payload_of_type(a.content, b"cbor") {
                            m.assertions.push((a.label, cbor));
                        }
                    }
                }
                "c2pa.claim.v2" | "c2pa.claim" => {
                    m.claim = payload_of_type(part.content, b"cbor");
                }
                "c2pa.signature" => {
                    m.signature = payload_of_type(part.content, b"cbor");
                }
                _ => {}
            }
        }
    }

    Some(m)
}
