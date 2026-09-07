//! imgscrub CLI。

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use imgscrub::inspect::{inspect, Inspection};
use imgscrub::jpeg::filter::{KeepSet, Options};
use imgscrub::scrub::{process_file, Destination, Error, Report};

const PREVENTION_HINT: &str = "\
Lightroom の Remove ツールで「生成AI」をオフにすれば、そもそも付与されません
          （修復ブラシ / コピースタンプも生成AI 経路を通りません）";

#[derive(Parser, Debug)]
#[command(
    name = "imgscrub",
    version,
    about = "JPEG から C2PA と追跡用メタデータを取り除く。画素は一切触らない",
    long_about = "JPEG のメタデータ衛生ツール。\n\n\
        必要な APPn セグメントだけを外科的に削除し、EXIF・ICC・IPTC はバイト単位で\n\
        保持する。スキャンデータ（画素）はデコードすらしないため無劣化。"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// 対象ファイルまたはディレクトリ
    paths: Vec<PathBuf>,

    /// 出力先ディレクトリ
    #[arg(short = 'o', long, value_name = "DIR")]
    out_dir: Option<PathBuf>,

    /// 入力を上書きする（既定は <name>_clean.jpg を隣に作る）
    #[arg(long, conflicts_with = "out_dir")]
    in_place: bool,

    /// APP11 のみ除去する。XMP も未知の APPn も触らない
    #[arg(long)]
    c2pa_only: bool,

    /// 除去しない対象をカンマ区切りで指定 [possible values: xmpmm, crs, mpf, unknown]
    #[arg(long, value_name = "LIST", value_delimiter = ',')]
    keep: Vec<String>,

    /// ディレクトリを再帰的に処理する
    #[arg(short = 'r', long)]
    recursive: bool,

    /// 変更内容だけ表示してファイルを書かない
    #[arg(short = 'n', long)]
    dry_run: bool,

    /// サマリのみ表示
    #[arg(short = 'q', long)]
    quiet: bool,

    /// 機械可読な JSON で出力する
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 診断のみ。ファイルを変更しない
    Inspect {
        paths: Vec<PathBuf>,
        /// 機械可読な JSON で出力する
        #[arg(long)]
        json: bool,
    },
    /// Lightroom Classic の書き出し後処理に登録する
    InstallLightroomAction {
        /// 既存の登録を確認なしで上書きする
        #[arg(long)]
        force: bool,
        /// 旧版の imgscrub.sh を指している書き出しプリセットを書き換える
        #[arg(long)]
        fix_presets: bool,
    },
    /// Lightroom Classic の書き出し後処理から削除する
    UninstallLightroomAction,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match &cli.command {
        Some(Command::Inspect { paths, json }) => run_inspect(paths, *json),
        Some(Command::InstallLightroomAction { force, fix_presets }) => {
            install_action(*force, *fix_presets)
        }
        Some(Command::UninstallLightroomAction) => uninstall_action(),
        None => run_scrub(&cli),
    }
}

/// 対象ファイルを集める。ディレクトリは `-r` の時だけ辿る。
fn collect(paths: &[PathBuf], recursive: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            if recursive {
                walk(p, &mut out);
            } else {
                eprintln!("{}: ディレクトリ（-r が必要）", p.display());
            }
        } else {
            out.push(p.clone());
        }
    }
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        eprintln!("{}: 読めない", dir.display());
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if is_jpeg(&p) {
            out.push(p);
        }
    }
}

fn is_jpeg(p: &Path) -> bool {
    p.extension()
        .map(|e| {
            let e = e.to_string_lossy().to_ascii_lowercase();
            e == "jpg" || e == "jpeg"
        })
        .unwrap_or(false)
}

fn run_scrub(cli: &Cli) -> ExitCode {
    if cli.paths.is_empty() {
        eprintln!("対象ファイルを指定してください（--help で使い方）");
        return ExitCode::from(2);
    }

    let keep = match KeepSet::parse(&cli.keep) {
        Ok(k) => k,
        Err(bad) => {
            eprintln!("--keep に不明な値: {bad}（xmpmm, crs, mpf, unknown のいずれか）");
            return ExitCode::from(2);
        }
    };
    if cli.c2pa_only && !cli.keep.is_empty() {
        eprintln!("--c2pa-only と --keep は併用できません");
        return ExitCode::from(2);
    }

    let dest = match (&cli.out_dir, cli.in_place) {
        (Some(d), _) => Destination::Dir(d.clone()),
        (None, true) => Destination::InPlace,
        (None, false) => Destination::Sibling,
    };
    let opts = Options {
        c2pa_only: cli.c2pa_only,
        keep,
    };

    let files = collect(&cli.paths, cli.recursive);
    let mut failures = 0usize;
    let mut changed = 0usize;
    let mut skipped = 0usize;
    let mut saved = 0usize;
    let mut json_rows: Vec<String> = Vec::new();

    for path in &files {
        if !is_jpeg(path) {
            skipped += 1;
            if !cli.quiet && !cli.json {
                println!("{}: unsupported format, skipped", path.display());
            }
            continue;
        }

        match process_file(path, &dest, &opts, cli.dry_run) {
            Ok((report, out_path)) => {
                if report.changed() {
                    changed += 1;
                    saved += report.saved();
                }
                if cli.json {
                    json_rows.push(report_json(path, &out_path, &report));
                } else if !cli.quiet {
                    print_report(path, &out_path, &report, cli.dry_run);
                }
            }
            Err(e) => {
                failures += 1;
                eprintln!("{}: {e}", path.display());
                if matches!(e, Error::Verify(_)) {
                    eprintln!("  → 入力ファイルは変更していない");
                }
            }
        }
    }

    if cli.json {
        println!("[{}]", json_rows.join(","));
    } else {
        let skip = if skipped > 0 {
            format!(", {skipped} skipped")
        } else {
            String::new()
        };
        println!("{changed} changed{skip}, -{saved} bytes");
    }

    if failures > 0 {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}

fn print_report(input: &Path, out: &Path, r: &Report, dry_run: bool) {
    let name = input.display();
    if !r.changed() {
        println!("{name}: already clean");
        return;
    }

    if dry_run {
        println!("{name} (dry-run)");
    } else if input == out {
        println!("{name}");
    } else {
        println!("{name} → {}", out.display());
    }

    for rm in &r.removals {
        println!("  削除: {} (-{} B)", rm.label, rm.bytes);
        for d in &rm.details {
            println!("        {d}");
        }
    }
    for w in &r.warnings {
        println!("  警告: {w}");
    }
    println!(
        "  {} B → {} B (-{} B)",
        r.bytes_before,
        r.bytes_after,
        r.saved()
    );
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn report_json(input: &Path, out: &Path, r: &Report) -> String {
    let removals: Vec<String> = r
        .removals
        .iter()
        .map(|rm| {
            let details: Vec<String> = rm.details.iter().map(|d| json_str(d)).collect();
            format!(
                "{{\"label\":{},\"bytes\":{},\"details\":[{}]}}",
                json_str(&rm.label),
                rm.bytes,
                details.join(",")
            )
        })
        .collect();
    let warnings: Vec<String> = r.warnings.iter().map(|w| json_str(w)).collect();
    format!(
        "{{\"input\":{},\"output\":{},\"changed\":{},\"bytes_before\":{},\"bytes_after\":{},\"removals\":[{}],\"warnings\":[{}]}}",
        json_str(&input.to_string_lossy()),
        json_str(&out.to_string_lossy()),
        r.changed(),
        r.bytes_before,
        r.bytes_after,
        removals.join(","),
        warnings.join(",")
    )
}

fn run_inspect(paths: &[PathBuf], json: bool) -> ExitCode {
    if paths.is_empty() {
        eprintln!("対象ファイルを指定してください");
        return ExitCode::from(2);
    }

    let mut failures = 0usize;
    let mut rows: Vec<String> = Vec::new();

    for path in paths {
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                failures += 1;
                continue;
            }
        };
        match inspect(&data) {
            Ok(i) => {
                if json {
                    rows.push(inspection_json(path, &i));
                } else {
                    print_inspection(path, &i);
                }
            }
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                failures += 1;
            }
        }
    }

    if json {
        println!("[{}]", rows.join(","));
    }

    if failures > 0 {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}

fn print_inspection(path: &Path, i: &Inspection) {
    println!(
        "{}  {}x{}  {} B",
        path.display(),
        i.width,
        i.height,
        i.bytes
    );
    println!();

    for s in &i.segments {
        println!("  {:<14} {:>8} B  {}", s.name, s.bytes, s.note);
    }

    if let Some(label) = &i.manifest_label {
        println!();
        println!("  C2PA マニフェスト: {label}");
        if let Some(c) = &i.claim {
            let name = c.generator_name.as_deref().unwrap_or("不明");
            let ver = c.generator_version.as_deref().unwrap_or("");
            let spec = c.spec_version.as_deref().unwrap_or("");
            println!("    生成元: {name} {ver}（C2PA 仕様 {spec}）");
        }
        if let Some(a) = &i.actions {
            for act in &a.actions {
                println!("    アクション: {}", act.name);
                if let Some(agent) = &act.software_agent {
                    println!("      ツール: {agent}");
                }
                if let Some(t) = &act.digital_source_type {
                    println!("      digitalSourceType: {}", short_source_type(t));
                }
                for (k, v) in &act.params {
                    println!("      {k}: {v}");
                }
            }
            if a.app_enforced {
                println!("    com.adobe.appEnforced: true  ← 書き出し設定に関係なく強制付与");
            }
        }
        if let Some(s) = &i.signer {
            let subject = s.subject_cn.as_deref().unwrap_or("不明");
            let issuer = s.issuer_cn.as_deref().unwrap_or("不明");
            println!("    署名: {subject}");
            println!("      発行者: {issuer}");
            if issuer.ends_with(".local") || issuer.contains("ephemeral") {
                println!("      → 一時 CA。トラストリスト外なので検証サイトでは");
                println!("        「クレデンシャルなし」と表示されるが、X はアサーションを読む");
            }
        }
    }

    println!();
    if i.ai_flagged() {
        println!("  ⚠ X が「AIで作成」を付ける原因: APP11 の digitalSourceType");
        println!("  ヒント: {PREVENTION_HINT}");
    } else if i.has_c2pa() {
        println!("  C2PA はあるが AI の宣言は含まれていない");
    } else if i.firefly {
        println!("  ⚠ C2PA はないが XMP に Firefly の痕跡がある");
    } else {
        println!("  AI 由来の申告は見つからなかった");
    }

    if !i.xmp_leaks.is_empty() {
        println!("  XMP に残る追跡情報: {}", i.xmp_leaks.join(", "));
    }
    if i.exif_privacy.any() {
        println!(
            "  EXIF に残るプライバシー情報: {}",
            i.exif_privacy.found().join(" / ")
        );
        println!("    → v0.1 では除去しない（--strip-exif-private は v0.2 予定）");
    }
    println!();
}

/// 長い IPTC の URI を末尾だけにする。
fn short_source_type(t: &str) -> String {
    match t.rsplit_once('/') {
        Some((_, last)) => format!("{last}（{t}）"),
        None => t.to_string(),
    }
}

fn inspection_json(path: &Path, i: &Inspection) -> String {
    let segs: Vec<String> = i
        .segments
        .iter()
        .map(|s| {
            format!(
                "{{\"name\":{},\"bytes\":{},\"note\":{}}}",
                json_str(&s.name),
                s.bytes,
                json_str(&s.note)
            )
        })
        .collect();
    let leaks: Vec<String> = i.xmp_leaks.iter().map(|l| json_str(l)).collect();
    let privacy: Vec<String> = i.exif_privacy.found().iter().map(|p| json_str(p)).collect();
    format!(
        "{{\"path\":{},\"width\":{},\"height\":{},\"bytes\":{},\"has_c2pa\":{},\"ai_flagged\":{},\"firefly\":{},\"segments\":[{}],\"xmp_leaks\":[{}],\"exif_privacy\":[{}]}}",
        json_str(&path.to_string_lossy()),
        i.width,
        i.height,
        i.bytes,
        i.has_c2pa(),
        i.ai_flagged(),
        i.firefly,
        segs.join(","),
        leaks.join(","),
        privacy.join(",")
    )
}

/// Lightroom Classic の書き出し後処理フォルダ。
///
/// `IMGSCRUB_EXPORT_ACTIONS_DIR` が設定されていればそれを使う。テストで実際の
/// Lightroom の設定を触らずに登録・削除を検証するためのもの。
fn export_actions_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("IMGSCRUB_EXPORT_ACTIONS_DIR") {
        return Some(PathBuf::from(d));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Adobe/Lightroom/Export Actions"))
}

/// Lightroom の書き出し後処理に置く AppleScript。
///
/// **シェルスクリプトでは動かない。** Lightroom Classic は Export Actions のアイテムを
/// LaunchServices 経由で「アプリケーションとして開く」ため、`.sh` は
/// `error -10811`（kLSNotAnApplicationErr）で起動されない。Apple Event の `odoc` を
/// 受け取れるアプリケーションバンドルである必要がある。
///
/// また imgscrub は**絶対パスで呼ぶ**。GUI アプリの PATH には Homebrew の
/// ディレクトリが含まれないため、`imgscrub` だけでは解決できない。
const ACTION_APPLESCRIPT: &str = r#"-- imgscrub — Lightroom Classic の書き出し後処理
-- imgscrub install-lightroom-action が生成する。手で編集しない。

property binaryPath : "@BINARY@"
property logPath : "@LOG@"

on run
	display dialog "これは Lightroom Classic の書き出し後処理から使うものです。" & return & return & "書き出しダイアログの「後処理」で imgscrub を選んでください。" buttons {"OK"} default button 1
end run

on open theFiles
	set stamp to do shell script "/bin/date '+%Y-%m-%d %H:%M:%S'"
	logLine(stamp & "  " & (count of theFiles) & " file(s)")
	repeat with f in theFiles
		try
			do shell script quoted form of binaryPath & " --in-place " & ¬
				quoted form of POSIX path of f & " >> " & quoted form of logPath & " 2>&1"
		on error errMsg
			logLine("  ERROR: " & errMsg)
		end try
	end repeat
end open

on logLine(t)
	do shell script "echo " & quoted form of t & " >> " & quoted form of logPath
end logLine
"#;

/// AppleScript に埋め込む imgscrub のパスを決める。
///
/// Homebrew は `/opt/homebrew/bin/imgscrub` を Cellar のバージョン入りパスへの symlink に
/// する。`canonicalize` するとそのバージョン入りパスになり、次の `brew upgrade` で
/// 後処理が壊れる。同じ実体を指す安定した symlink があればそちらを埋め込む。
fn stable_binary_path() -> std::io::Result<PathBuf> {
    let real = std::env::current_exe()?.canonicalize()?;
    for candidate in ["/opt/homebrew/bin/imgscrub", "/usr/local/bin/imgscrub"] {
        let c = Path::new(candidate);
        if c.canonicalize().is_ok_and(|r| r == real) {
            return Ok(c.to_path_buf());
        }
    }
    Ok(real)
}

/// Lightroom Classic の書き出しプリセットの置き場所。
fn export_presets_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("IMGSCRUB_EXPORT_PRESETS_DIR") {
        return Some(PathBuf::from(d));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Adobe/Lightroom/Export Presets"))
}

/// `.lrtemplate` を再帰的に集める。
fn collect_presets(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_presets(&p, out);
        } else if p.extension().is_some_and(|x| x == "lrtemplate") {
            out.push(p);
        }
    }
}

/// 旧版のシェルスクリプトを後処理に指定しているプリセットを探す。
///
/// プリセットは後処理を**絶対パスで**記録する。`.sh` は LaunchServices から起動できない
/// ので、その参照が残っているプリセットは黙って何もしない書き出しになる。
fn presets_referencing_legacy(legacy: &Path) -> Vec<PathBuf> {
    let Some(dir) = export_presets_dir() else {
        return Vec::new();
    };
    let mut files = Vec::new();
    collect_presets(&dir, &mut files);

    let needle = legacy.to_string_lossy().to_string();
    files
        .into_iter()
        .filter(|p| {
            std::fs::read_to_string(p)
                .map(|s| s.contains(&needle))
                .unwrap_or(false)
        })
        .collect()
}

/// プリセットの後処理の参照を `.sh` から `.app` に書き換える。
fn fix_preset(path: &Path, legacy: &Path, app: &Path) -> std::io::Result<()> {
    let text = std::fs::read_to_string(path)?;
    let fixed = text.replace(&*legacy.to_string_lossy(), &app.to_string_lossy());
    if fixed == text {
        return Ok(());
    }
    // 書き換える前に元を残す
    let backup = path.with_extension("lrtemplate.imgscrub-backup");
    if !backup.exists() {
        std::fs::copy(path, &backup)?;
    }
    std::fs::write(path, fixed)
}

/// 後処理のログの置き場所。
fn action_log_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Logs/imgscrub-lightroom.log"))
}

fn install_action(force: bool, fix_presets: bool) -> ExitCode {
    let Some(dir) = export_actions_dir() else {
        eprintln!("HOME が取得できません");
        return ExitCode::from(2);
    };
    if !dir.exists() {
        eprintln!("Lightroom Classic の書き出し後処理フォルダが見つかりません:");
        eprintln!("  {}", dir.display());
        eprintln!("Lightroom Classic を一度起動すると作られます");
        return ExitCode::from(2);
    }

    // GUI アプリの PATH には頼れないので絶対パスを埋め込む
    let binary = match stable_binary_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("自分の実行パスを解決できません: {e}");
            return ExitCode::from(2);
        }
    };
    let Some(log) = action_log_path() else {
        eprintln!("HOME が取得できません");
        return ExitCode::from(2);
    };

    let app = dir.join("imgscrub.app");
    if app.exists() && !force {
        eprintln!("既に登録されています: {}", app.display());
        eprintln!("上書きするなら --force");
        return ExitCode::from(2);
    }

    let script = ACTION_APPLESCRIPT
        .replace("@BINARY@", &binary.to_string_lossy())
        .replace("@LOG@", &log.to_string_lossy());

    // 固定名にすると同時に 2 つ走ったときに互いのファイルを消し合う
    let tmp = std::env::temp_dir().join(format!(
        "imgscrub-action-{}.applescript",
        std::process::id()
    ));
    if let Err(e) = std::fs::write(&tmp, &script) {
        eprintln!("{}: {e}", tmp.display());
        return ExitCode::from(2);
    }

    if app.exists() {
        let _ = std::fs::remove_dir_all(&app);
    }

    // osacompile は macOS 標準。odoc を受け取れるアプリケーションバンドルを作る
    let out = std::process::Command::new("/usr/bin/osacompile")
        .arg("-o")
        .arg(&app)
        .arg(&tmp)
        .output();
    let _ = std::fs::remove_file(&tmp);

    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            eprintln!("osacompile が失敗しました:");
            eprintln!("{}", String::from_utf8_lossy(&o.stderr));
            return ExitCode::from(2);
        }
        Err(e) => {
            eprintln!("osacompile を実行できません: {e}");
            return ExitCode::from(2);
        }
    }

    // 旧版のシェルスクリプトが残っていると起動できない選択肢が並ぶので消す
    let legacy = dir.join("imgscrub.sh");
    let had_legacy = legacy.exists();
    if had_legacy {
        let _ = std::fs::remove_file(&legacy);
    }

    println!("登録しました: {}", app.display());
    println!("  呼び出す imgscrub: {}", binary.display());
    println!("  ログ: {}", log.display());
    if had_legacy {
        println!("  旧版の imgscrub.sh を削除しました（LaunchServices から起動できないため）");
    }

    // プリセットは後処理を絶対パスで持つ。旧 .sh を指したままだと黙って何もしない
    let stale = presets_referencing_legacy(&legacy);
    if !stale.is_empty() {
        println!();
        println!("⚠ 旧版の imgscrub.sh を指している書き出しプリセットがあります:");
        for p in &stale {
            let name = p.file_stem().unwrap_or_default().to_string_lossy();
            println!("    {name}");
        }
        if fix_presets {
            let mut fixed = 0usize;
            for p in &stale {
                match fix_preset(p, &legacy, &app) {
                    Ok(()) => fixed += 1,
                    Err(e) => eprintln!("    {}: {e}", p.display()),
                }
            }
            println!(
                "  {fixed} 件を imgscrub.app に書き換えました（元は .imgscrub-backup に保存）"
            );
        } else {
            println!("  このままでは後処理が何も実行されません。");
            println!("  --fix-presets で書き換えられます（元はバックアップします）。");
        }
    }

    println!();
    println!("この後の手順:");
    println!("  1. Lightroom Classic を再起動する");
    println!("     （Export Actions フォルダは起動時にしか読まれないため）");
    println!("  2. 書き出しダイアログの「後処理」で「imgscrub」を選ぶ");
    println!("  3. プリセットを使っているなら上書き保存する");
    println!();
    println!("動いたかどうかは {} で確認できます。", log.display());
    ExitCode::SUCCESS
}

fn uninstall_action() -> ExitCode {
    let Some(dir) = export_actions_dir() else {
        eprintln!("HOME が取得できません");
        return ExitCode::from(2);
    };

    let app = dir.join("imgscrub.app");
    let legacy = dir.join("imgscrub.sh");
    let mut removed = false;

    if app.exists() {
        match std::fs::remove_dir_all(&app) {
            Ok(()) => {
                println!("削除しました: {}", app.display());
                removed = true;
            }
            Err(e) => {
                eprintln!("{}: {e}", app.display());
                return ExitCode::from(2);
            }
        }
    }
    if legacy.exists() {
        match std::fs::remove_file(&legacy) {
            Ok(()) => {
                println!("削除しました: {}", legacy.display());
                removed = true;
            }
            Err(e) => {
                eprintln!("{}: {e}", legacy.display());
                return ExitCode::from(2);
            }
        }
    }

    if !removed {
        println!("登録されていません");
    }
    ExitCode::SUCCESS
}
