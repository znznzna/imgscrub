//! 出力の検証。
//!
//! 画素の無劣化はデコードせずに機械的に確認できる。SOS セグメントのバイト列が
//! 入力と完全に一致すれば、再エンコードが起きていないことが保証される。

use super::segment::{check_eoi, scan, scan_data, JpegError};

/// 検証で見つかった不整合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// 出力が JPEG として走査できない
    Unparsable(JpegError),
    /// スキャンデータが入力と一致しない（＝画素が変わっている）
    ScanDataMismatch { input: usize, output: usize },
    /// 出力が EOI で終わっていない
    MissingEoi,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unparsable(e) => write!(f, "出力を再走査できない: {e}"),
            Self::ScanDataMismatch { input, output } => write!(
                f,
                "スキャンデータが一致しない（入力 {input} B / 出力 {output} B）"
            ),
            Self::MissingEoi => write!(f, "出力が EOI で終わっていない"),
        }
    }
}

impl std::error::Error for VerifyError {}

/// 出力が入力から画素を変えずに作られたことを検証する。
///
/// 書き出し前にこれを通し、失敗したら出力を破棄して入力を無変更のまま残す。
pub fn verify(input: &[u8], output: &[u8]) -> Result<(), VerifyError> {
    let in_segs = scan(input).map_err(VerifyError::Unparsable)?;
    let out_segs = scan(output).map_err(VerifyError::Unparsable)?;

    check_eoi(output).map_err(|_| VerifyError::MissingEoi)?;

    let a = scan_data(input, &in_segs);
    let b = scan_data(output, &out_segs);
    if a != b {
        return Err(VerifyError::ScanDataMismatch {
            input: a.len(),
            output: b.len(),
        });
    }

    Ok(())
}
