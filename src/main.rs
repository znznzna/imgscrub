//! imgscrub CLI。
//!
//! Phase 2 時点の最小構成。`inspect` サブコマンドと `-r` / `--json` / `--keep` は
//! Phase 4-5 で追加する。

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use imgscrub::jpeg::filter::Options;
use imgscrub::scrub::{process_file, Destination, Error, Report};

#[derive(Parser, Debug)]
#[command(
    name = "imgscrub",
    version,
    about = "JPEG から C2PA と追跡用メタデータを取り除く（画素は一切触らない）"
)]
struct Cli {
    /// 対象ファイル
    #[arg(required = true)]
    paths: Vec<PathBuf>,

    /// 出力先ディレクトリ
    #[arg(short = 'o', long)]
    out_dir: Option<PathBuf>,

    /// 入力を上書きする（既定は <name>_clean.jpg を隣に作る）
    #[arg(long, conflicts_with = "out_dir")]
    in_place: bool,

    /// APP11 のみ除去する。XMP も未知の APPn も触らない
    #[arg(long)]
    c2pa_only: bool,

    /// 変更内容だけ表示してファイルを書かない
    #[arg(short = 'n', long)]
    dry_run: bool,

    /// サマリのみ表示
    #[arg(short = 'q', long)]
    quiet: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let dest = match (&cli.out_dir, cli.in_place) {
        (Some(d), _) => Destination::Dir(d.clone()),
        (None, true) => Destination::InPlace,
        (None, false) => Destination::Sibling,
    };
    let opts = Options {
        c2pa_only: cli.c2pa_only,
    };

    let mut failures = 0usize;
    let mut changed = 0usize;
    let mut saved = 0usize;

    for path in &cli.paths {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(ext.as_str(), "jpg" | "jpeg") {
            if !cli.quiet {
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
                if !cli.quiet {
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

    if !cli.quiet || cli.paths.len() > 1 {
        println!("{changed} changed, -{saved} bytes");
    }

    if failures > 0 {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}

fn print_report(input: &std::path::Path, out: &std::path::Path, r: &Report, dry_run: bool) {
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
