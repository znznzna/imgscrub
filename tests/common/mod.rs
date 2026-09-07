//! テスト共通ヘルパ。
//!
//! 統合テストはファイルごとに独立したクレートになるため、一部のテストでしか
//! 使わない項目が dead_code として警告される。共有ヘルパなので許容する。

#![allow(dead_code)]

use std::path::PathBuf;

/// フィクスチャを読み込む。
pub fn fixture(name: &str) -> Vec<u8> {
    let p = path(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("フィクスチャが読めない {}: {e}", p.display()))
}

/// フィクスチャのパス。
pub fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// 全フィクスチャの名前。
pub const ALL: &[&str] = &[
    "lrc_firefly.jpg",
    "lrc_clean.jpg",
    "camera_mpf.jpg",
    "no_exif.jpg",
];
