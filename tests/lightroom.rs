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

/// プリセットは後処理を絶対パスで持つ。旧 `.sh` を指したままだと黙って何もしない
/// 書き出しになるので、登録時に検出する。
#[test]
fn stale_presets_are_detected() {
    let dir = workdir("presets");
    let presets = workdir("presets-store");
    let legacy = dir.join("imgscrub.sh");
    std::fs::write(&legacy, "#!/bin/sh\n").unwrap();

    let sub = presets.join("User Presets");
    std::fs::create_dir_all(&sub).unwrap();
    let preset = sub.join("Scrub out.lrtemplate");
    std::fs::write(
        &preset,
        format!(
            "s = {{\n\texport_postProcessing = \"{}\",\n}}\n",
            legacy.display()
        ),
    )
    .unwrap();

    let out = Command::new(bin())
        .arg("install-lightroom-action")
        .env("IMGSCRUB_EXPORT_ACTIONS_DIR", &dir)
        .env("IMGSCRUB_EXPORT_PRESETS_DIR", &presets)
        .output()
        .unwrap();
    assert!(out.status.success());

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Scrub out"),
        "プリセット名が出ない: {stdout}"
    );
    assert!(stdout.contains("--fix-presets"), "修復方法の案内がない");
    // 検出だけなので書き換えていない
    assert!(std::fs::read_to_string(&preset)
        .unwrap()
        .contains("imgscrub.sh"));
}

#[test]
fn fix_presets_rewrites_and_backs_up() {
    let dir = workdir("fix");
    let presets = workdir("fix-store");
    let legacy = dir.join("imgscrub.sh");
    std::fs::write(&legacy, "#!/bin/sh\n").unwrap();

    let preset = presets.join("p.lrtemplate");
    std::fs::write(
        &preset,
        format!("export_postProcessing = \"{}\",\n", legacy.display()),
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["install-lightroom-action", "--fix-presets"])
        .env("IMGSCRUB_EXPORT_ACTIONS_DIR", &dir)
        .env("IMGSCRUB_EXPORT_PRESETS_DIR", &presets)
        .output()
        .unwrap();
    assert!(out.status.success());

    let after = std::fs::read_to_string(&preset).unwrap();
    assert!(
        after.contains("imgscrub.app"),
        "書き換えられていない: {after}"
    );
    assert!(!after.contains("imgscrub.sh"));

    let backup = preset.with_extension("lrtemplate.imgscrub-backup");
    assert!(backup.exists(), "バックアップがない");
    assert!(std::fs::read_to_string(&backup)
        .unwrap()
        .contains("imgscrub.sh"));
}

/// Homebrew の symlink を埋め込む。Cellar のバージョン入りパスを埋めると
/// 次の brew upgrade で後処理が壊れる。
#[test]
fn brew_symlink_is_preferred_over_cellar_path() {
    let dir = workdir("symlink");
    let out = run(&dir, &["install-lightroom-action"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);

    let line = stdout
        .lines()
        .find(|l| l.contains("呼び出す imgscrub:"))
        .expect("パスが報告される");
    assert!(
        !line.contains("/Cellar/"),
        "Cellar のバージョン入りパスが埋め込まれた: {line}"
    );
}
