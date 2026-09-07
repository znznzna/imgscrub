//! Lightroom Classic の書き出し後処理への登録のテスト（macOS のみ）。
//!
//! **シェルスクリプトでは動かない。** Lightroom は Export Actions のアイテムを
//! LaunchServices 経由でアプリケーションとして開くため、`.sh` は
//! `error -10811`（kLSNotAnApplicationErr）になる。ここで固定しているのは
//! 「生成物が本当にアプリケーションバンドルであること」。
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_imgscrub")
}

fn workdir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("lightroom")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .env("IMGSCRUB_EXPORT_ACTIONS_DIR", dir)
        .output()
        .expect("バイナリを起動できる")
}

#[test]
fn install_creates_an_application_bundle() {
    let dir = workdir("install");
    let out = run(&dir, &["install-lightroom-action"]);
    assert!(
        out.status.success(),
        "登録に失敗: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let app = dir.join("imgscrub.app");
    assert!(app.is_dir(), "アプリケーションバンドルが作られていない");

    // LaunchServices が起動できるのはこの形だけ
    let plist = app.join("Contents/Info.plist");
    assert!(plist.is_file(), "Info.plist がない");
    let dumped = Command::new("/usr/bin/plutil")
        .args(["-p"])
        .arg(&plist)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&dumped.stdout);
    assert!(
        text.contains("\"CFBundlePackageType\" => \"APPL\""),
        "APPL として宣言されていない: {text}"
    );
    assert!(
        app.join("Contents/MacOS").is_dir(),
        "実行可能ファイルのディレクトリがない"
    );

    // 呼び出す imgscrub は絶対パスで埋め込まれている（GUI の PATH に頼れないため）
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("呼び出す imgscrub: /"),
        "絶対パスが報告されていない: {stdout}"
    );
}

#[test]
fn install_refuses_to_overwrite_without_force() {
    let dir = workdir("force");
    assert!(run(&dir, &["install-lightroom-action"]).status.success());

    let again = run(&dir, &["install-lightroom-action"]);
    assert!(!again.status.success(), "確認なしで上書きされた");

    let forced = run(&dir, &["install-lightroom-action", "--force"]);
    assert!(forced.status.success(), "--force で上書きできない");
}

/// 旧版のシェルスクリプトは、起動できない選択肢として残るので登録時に消す。
#[test]
fn install_removes_the_legacy_shell_script() {
    let dir = workdir("legacy");
    let legacy = dir.join("imgscrub.sh");
    std::fs::write(&legacy, "#!/bin/sh\nexec imgscrub \"$@\"\n").unwrap();

    let out = run(&dir, &["install-lightroom-action"]);
    assert!(out.status.success());
    assert!(!legacy.exists(), "旧版の .sh が残っている");
    assert!(String::from_utf8_lossy(&out.stdout).contains("imgscrub.sh"));
}

#[test]
fn uninstall_removes_both_forms() {
    let dir = workdir("uninstall");
    assert!(run(&dir, &["install-lightroom-action"]).status.success());
    std::fs::write(dir.join("imgscrub.sh"), "#!/bin/sh\n").unwrap();

    let out = run(&dir, &["uninstall-lightroom-action"]);
    assert!(out.status.success());
    assert!(!dir.join("imgscrub.app").exists());
    assert!(!dir.join("imgscrub.sh").exists());

    // 2 回目は何もせず成功する
    let again = run(&dir, &["uninstall-lightroom-action"]);
    assert!(again.status.success());
    assert!(String::from_utf8_lossy(&again.stdout).contains("登録されていません"));
}

#[test]
fn missing_directory_is_reported() {
    let dir = workdir("missing").join("does-not-exist");
    let out = run(&dir, &["install-lightroom-action"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("見つかりません"));
}
