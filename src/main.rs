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
        /// 既存のスクリプトを確認なしで上書きする
        #[arg(long)]
        force: bool,
    },
    /// Lightroom Classic の書き出し後処理から削除する
    UninstallLightroomAction,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match &cli.command {
        Some(Command::Inspect { paths, json }) => run_inspect(paths, *json),
        Some(Command::InstallLightroomAction { force }) => install_action(*force),
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
fn export_actions_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Adobe/Lightroom/Export Actions"))
}

const ACTION_SCRIPT: &str = "\
#!/bin/sh
# imgscrub — Lightroom Classic の書き出し後処理
#
# 書き出したファイルが引数で渡される。上書きで処理し、対応しない
# フォーマットは黙って飛ばす（書き出し全体を失敗させないため）。
exec imgscrub --in-place --quiet \"$@\"
";

fn install_action(force: bool) -> ExitCode {
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

    let script = dir.join("imgscrub.sh");
    if script.exists() && !force {
        eprintln!("既に登録されています: {}", script.display());
        eprintln!("上書きするなら --force");
        return ExitCode::from(2);
    }

    if let Err(e) = std::fs::write(&script, ACTION_SCRIPT) {
        eprintln!("{}: {e}", script.display());
        return ExitCode::from(2);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755));
    }

    println!("登録しました: {}", script.display());
    println!();
    println!("Lightroom Classic の書き出しダイアログの「後処理」で");
    println!("「imgscrub」を選ぶと、書き出し後に自動で実行されます。");
    println!("（imgscrub が PATH 上にある必要があります）");
    ExitCode::SUCCESS
}

fn uninstall_action() -> ExitCode {
    let Some(dir) = export_actions_dir() else {
        eprintln!("HOME が取得できません");
        return ExitCode::from(2);
    };
    let script = dir.join("imgscrub.sh");
    if !script.exists() {
        println!("登録されていません");
        return ExitCode::SUCCESS;
    }
    match std::fs::remove_file(&script) {
        Ok(()) => {
            println!("削除しました: {}", script.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}: {e}", script.display());
            ExitCode::from(2)
        }
    }
}
