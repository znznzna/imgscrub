//! JPEG マーカーセグメントの走査。
//!
//! SOS（Start of Scan）に到達した時点で走査を打ち切る。スキャンデータの中には
//! バイトスタッフィング（`FF 00`）とリスタートマーカー（`FF D0`–`FF D7`）が現れるため、
//! そこを走査し続けると存在しないセグメントを誤検出する。

use std::fmt;
use std::ops::Range;

/// Start of Image
pub const SOI: u8 = 0xD8;
/// End of Image
pub const EOI: u8 = 0xD9;
/// Start of Scan
pub const SOS: u8 = 0xDA;
/// Temporary（算術符号用。長さフィールドを持たない）
pub const TEM: u8 = 0x01;

/// 長さフィールドを持たないマーカーか。
fn is_standalone(marker: u8) -> bool {
    marker == TEM || (0xD0..=0xD7).contains(&marker)
}

/// APPn セグメントの識別子。ペイロード先頭のシグネチャで判別する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum App {
    /// APP0 `JFIF`
    Jfif,
    /// APP1 `Exif\0\0`
    Exif,
    /// APP1 `http://ns.adobe.com/xap/1.0/\0`
    Xmp,
    /// APP1 `http://ns.adobe.com/xmp/extension/\0`（拡張 XMP）
    XmpExtension,
    /// APP2 `ICC_PROFILE\0`
    Icc,
    /// APP2 `MPF\0`（Multi-Picture Format。絶対オフセットを含む）
    Mpf,
    /// APP11 `JP`（JUMBF。C2PA マニフェストの容れ物）
    Jumbf,
    /// APP13 `Photoshop 3.0\0`（IPTC / Photoshop リソース）
    Photoshop,
    /// APP14 `Adobe`（ColorTransform を宣言する。削除すると色解釈が変わる）
    Adobe,
    /// 上記以外の APPn
    Unknown(u8),
}

/// 1 つのマーカーセグメント。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// マーカーの種別（`0xFF` の次のバイト）
    pub marker: u8,
    /// `0xFF` の位置
    pub start: usize,
    /// 次のセグメントの開始位置。SOS の場合はデータ末尾
    pub end: usize,
    /// 長さフィールドを除いたペイロードの範囲。長さフィールドを持たない場合は `None`
    pub payload: Option<Range<usize>>,
}

impl Segment {
    /// マーカーを含むセグメント全体のバイト数。
    ///
    /// セグメントは最小 2 バイト（マーカーのみ）で空になり得ないため `len`/`is_empty` の
    /// 対を持たせる意味がない。`size` という名前にしている。
    pub fn size(&self) -> usize {
        self.end - self.start
    }

    /// セグメント全体のバイト列。
    pub fn bytes<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        &data[self.start..self.end]
    }

    /// ペイロードのバイト列。長さフィールドを持たないマーカーでは空。
    pub fn payload_bytes<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        match &self.payload {
            Some(r) => &data[r.clone()],
            None => &[],
        }
    }

    /// APPn セグメントか。
    pub fn is_app(&self) -> bool {
        (0xE0..=0xEF).contains(&self.marker)
    }

    /// APPn の n。APPn でなければ `None`。
    pub fn app_index(&self) -> Option<u8> {
        if self.is_app() {
            Some(self.marker - 0xE0)
        } else {
            None
        }
    }

    /// APPn の種別をペイロードのシグネチャから判別する。
    pub fn app_kind(&self, data: &[u8]) -> Option<App> {
        let n = self.app_index()?;
        let p = self.payload_bytes(data);
        let kind = match n {
            0 if p.starts_with(b"JFIF\0") => App::Jfif,
            1 if p.starts_with(b"Exif\0") => App::Exif,
            1 if p.starts_with(b"http://ns.adobe.com/xap/1.0/\0") => App::Xmp,
            1 if p.starts_with(b"http://ns.adobe.com/xmp/extension/\0") => App::XmpExtension,
            2 if p.starts_with(b"ICC_PROFILE\0") => App::Icc,
            2 if p.starts_with(b"MPF\0") => App::Mpf,
            // APP11 は JUMBF 専用。共通識別子 'JP' + Box Instance(2) + Packet seq(4) が続く
            11 if p.starts_with(b"JP") => App::Jumbf,
            13 if p.starts_with(b"Photoshop 3.0\0") => App::Photoshop,
            14 if p.starts_with(b"Adobe") => App::Adobe,
            _ => App::Unknown(n),
        };
        Some(kind)
    }
}

/// 走査中に検出した構造上の異常。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JpegError {
    /// 先頭が `FF D8` でない
    MissingSoi,
    /// 長さフィールドがデータ末尾を超えている
    TruncatedSegment { offset: usize, marker: u8 },
    /// 長さフィールドが 2 未満（自身の 2 バイトを含むため 2 が最小）
    BadLength { offset: usize, marker: u8, len: u16 },
    /// SOS に到達しないままデータが終わった
    MissingSos,
    /// 末尾が `FF D9` でない（スキャンデータが切り詰められている）
    MissingEoi,
}

impl fmt::Display for JpegError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSoi => write!(f, "JPEG ではない（先頭が FF D8 でない）"),
            Self::TruncatedSegment { offset, marker } => write!(
                f,
                "セグメントが切り詰められている: マーカー FF{marker:02X} at {offset}"
            ),
            Self::BadLength {
                offset,
                marker,
                len,
            } => write!(
                f,
                "長さフィールドが不正: マーカー FF{marker:02X} at {offset}, len={len}"
            ),
            Self::MissingSos => write!(f, "スキャンデータ（SOS）が見つからない"),
            Self::MissingEoi => write!(f, "末尾が FF D9 でない（切り詰められている）"),
        }
    }
}

impl std::error::Error for JpegError {}

/// JPEG のマーカーセグメントを列挙する。
///
/// SOS セグメントはスキャンデータ全体を含む単一のセグメントとして返し、そこで走査を終える。
/// スキャンデータの内部は解釈しない。
pub fn scan(data: &[u8]) -> Result<Vec<Segment>, JpegError> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != SOI {
        return Err(JpegError::MissingSoi);
    }

    let mut out = Vec::new();
    let mut i = 2usize;

    while i + 1 < data.len() {
        // マーカーの前には任意個の 0xFF パディングが入りうる
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }

        if is_standalone(marker) {
            out.push(Segment {
                marker,
                start: i,
                end: i + 2,
                payload: None,
            });
            i += 2;
            continue;
        }

        if marker == SOS {
            out.push(Segment {
                marker,
                start: i,
                end: data.len(),
                payload: None,
            });
            return Ok(out);
        }

        if marker == EOI {
            out.push(Segment {
                marker,
                start: i,
                end: i + 2,
                payload: None,
            });
            i += 2;
            continue;
        }

        // 長さフィールドを持つセグメント
        if i + 4 > data.len() {
            return Err(JpegError::TruncatedSegment { offset: i, marker });
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]);
        if len < 2 {
            return Err(JpegError::BadLength {
                offset: i,
                marker,
                len,
            });
        }
        let end = i + 2 + len as usize;
        if end > data.len() {
            return Err(JpegError::TruncatedSegment { offset: i, marker });
        }
        out.push(Segment {
            marker,
            start: i,
            end,
            payload: Some(i + 4..end),
        });
        i = end;
    }

    Err(JpegError::MissingSos)
}

/// 末尾が EOI で終わっているかを検証する。
///
/// `scan` はスキャンデータの内部を見ないため、SOS の途中で切り詰められたファイルも
/// 走査自体は成功する。切り詰めの検出はこの関数で行う。
pub fn check_eoi(data: &[u8]) -> Result<(), JpegError> {
    if data.len() >= 2 && data[data.len() - 2] == 0xFF && data[data.len() - 1] == EOI {
        Ok(())
    } else {
        Err(JpegError::MissingEoi)
    }
}

/// SOS セグメント（スキャンデータ）のバイト列を返す。
///
/// 出力の検証で入力と一致することを確認するために使う。画素無劣化の機械的な保証になる。
pub fn scan_data<'a>(data: &'a [u8], segs: &[Segment]) -> &'a [u8] {
    match segs.iter().find(|s| s.marker == SOS) {
        Some(s) => s.bytes(data),
        None => &[],
    }
}
