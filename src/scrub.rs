//! 処理の本体。バイト列の変換とファイルへの安全な適用。

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::jpeg::filter::{decide, Action, DropReason, Options};
use crate::jpeg::segment::{check_eoi, scan, App, JpegError, Segment};
use crate::jpeg::verify::{verify, VerifyError};
use crate::xmp;

/// 1 件の除去。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    /// 人間向けのラベル
    pub label: String,
    /// 減ったバイト数
    pub bytes: usize,
    /// 内訳（XMP のプロパティ名など）
    pub details: Vec<String>,
}

impl Removal {
    fn new(label: impl Into<String>, bytes: usize) -> Self {
        Self {
            label: label.into(),
            bytes,
            details: Vec::new(),
        }
    }
}

/// 処理の結果。
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub removals: Vec<Removal>,
    /// 呼び出し側に伝えるべき注意
    pub warnings: Vec<String>,
    pub bytes_before: usize,
    pub bytes_after: usize,
}

impl Report {
    /// 何か変わったか。
    pub fn changed(&self) -> bool {
        !self.removals.is_empty()
    }

    /// 減ったバイト数。
    pub fn saved(&self) -> usize {
        self.bytes_before.saturating_sub(self.bytes_after)
    }
}

/// 処理中のエラー。
#[derive(Debug)]
pub enum Error {
    /// JPEG として読めない
    Jpeg(JpegError),
    /// 出力の検証に失敗した（入力は無変更のまま残る）
    Verify(VerifyError),
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Jpeg(e) => write!(f, "{e}"),
            Self::Verify(e) => write!(f, "検証に失敗（ファイルは変更していない）: {e}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// バイト列を処理する。ファイルには触らない。
pub fn process(data: &[u8], opts: &Options) -> Result<(Vec<u8>, Report), Error> {
    let segs = scan(data).map_err(Error::Jpeg)?;
    check_eoi(data).map_err(Error::Jpeg)?;

    let actions = decide(data, &segs, opts);

    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(&data[..2]); // SOI

    let mut report = Report {
        bytes_before: data.len(),
        ..Default::default()
    };

    for (seg, action) in segs.iter().zip(&actions) {
        match action {
            Action::Keep => out.extend_from_slice(seg.bytes(data)),

            Action::Drop(reason) => {
                report
                    .removals
                    .push(Removal::new(label_for(reason, seg.app_index()), seg.size()));
            }

            Action::Rewrite => rewrite_xmp(data, seg, opts, &mut out, &mut report),
        }

        if seg.app_kind(data) == Some(App::XmpExtension) {
            report
                .warnings
                .push("拡張 XMP があるため XMP は無編集で保持した".to_string());
        }
    }

    report.bytes_after = out.len();

    verify(data, &out).map_err(Error::Verify)?;

    Ok((out, report))
}

/// APP1/XMP をプロパティ単位で書き換えて出力に足す。
///
/// 編集できない場合（UTF-8 でない・XML が壊れている・再直列化で膨らむ）は無編集で通し、
/// 警告を残す。プロパティが残らなくなった場合はセグメントごと落とす。
fn rewrite_xmp(data: &[u8], seg: &Segment, opts: &Options, out: &mut Vec<u8>, report: &mut Report) {
    let payload = seg.payload_bytes(data);
    let xml = &payload[xmp::XMP_HEADER.len()..];

    let Some((new_xml, outcome)) = xmp::strip(xml, &opts.keep.to_xmp_keep()) else {
        out.extend_from_slice(seg.bytes(data));
        report
            .warnings
            .push("XMP を編集できなかったため無編集で保持した".to_string());
        return;
    };

    if outcome.removed.is_empty() && new_xml.len() == xml.len() {
        out.extend_from_slice(seg.bytes(data));
        return;
    }

    let removed_bytes = seg.size() - (xmp::XMP_HEADER.len() + new_xml.len() + 4);

    let details: Vec<String> = outcome
        .removed
        .iter()
        .map(|r| format!("{} — {}", r.name, r.why))
        .collect();

    if outcome.is_empty {
        let mut rm = Removal::new("APP1/XMP (残るプロパティがないため全体を削除)", seg.size());
        rm.details = details;
        report.removals.push(rm);
        return;
    }

    let mut rm = Removal::new(
        format!("APP1/XMP ({} プロパティ + パディング)", details.len()),
        removed_bytes,
    );
    rm.details = details;
    report.removals.push(rm);

    let body_len = xmp::XMP_HEADER.len() + new_xml.len();
    // セグメント長フィールドは自身の 2 バイトを含む
    let seg_len = (body_len + 2) as u16;
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&seg_len.to_be_bytes());
    out.extend_from_slice(xmp::XMP_HEADER);
    out.extend_from_slice(&new_xml);
}

fn label_for(reason: &DropReason, app: Option<u8>) -> String {
    match app {
        Some(n) => format!("APP{n} ({reason})"),
        None => reason.to_string(),
    }
}

/// 出力先の決め方。
#[derive(Debug, Clone)]
pub enum Destination {
    /// 入力を上書きする
    InPlace,
    /// 入力の隣に `<stem>_clean.<ext>` を作る
    Sibling,
    /// 指定ディレクトリに同名で書く
    Dir(PathBuf),
}

impl Destination {
    /// 入力パスに対する出力パスを返す。
    pub fn resolve(&self, input: &Path) -> PathBuf {
        match self {
            Self::InPlace => input.to_path_buf(),
            Self::Dir(d) => d.join(input.file_name().unwrap_or_default()),
            Self::Sibling => {
                let stem = input.file_stem().unwrap_or_default().to_string_lossy();
                let ext = input.extension().map(|e| e.to_string_lossy().to_string());
                let name = match ext {
                    Some(e) => format!("{stem}_clean.{e}"),
                    None => format!("{stem}_clean"),
                };
                input.with_file_name(name)
            }
        }
    }
}

/// ファイルを処理する。
///
/// 一時ファイルに書いて検証を通してから atomic rename する。検証に失敗した場合は
/// 一時ファイルを破棄し、入力・出力のどちらも変更しない。
pub fn process_file(
    input: &Path,
    dest: &Destination,
    opts: &Options,
    dry_run: bool,
) -> Result<(Report, PathBuf), Error> {
    let data = fs::read(input)?;
    let (out, report) = process(&data, opts)?;
    let out_path = dest.resolve(input);

    if dry_run || (!report.changed() && matches!(dest, Destination::InPlace)) {
        return Ok((report, out_path));
    }

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // 同一ディレクトリの一時ファイルに書く（rename を atomic にするため）
    let tmp = out_path.with_extension(format!(
        "{}.imgscrub-tmp",
        out_path
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default()
    ));
    fs::write(&tmp, &out)?;

    // 書き込んだ結果をもう一度読み直して検証する
    match fs::read(&tmp).map_err(Error::Io).and_then(|written| {
        verify(&data, &written).map_err(Error::Verify)?;
        Ok(())
    }) {
        Ok(()) => {
            fs::rename(&tmp, &out_path)?;
            Ok((report, out_path))
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}
