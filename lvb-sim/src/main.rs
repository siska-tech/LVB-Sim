// LVB-Sim: 将来の拡張のために定義されたが現時点では未使用の項目を許容する
#![allow(dead_code)]

/// Lumen VIA Beeper Simulator — CLI エントリポイント
///
/// 使用例:
///   lvb-sim input.yaml --out output.wav
///   lvb-sim input.mmml --mode BassLock --arp-rate 240 --out demo.wav
///   lvb-sim benchmark protodome.mmml --compare basslock,pseudo4 --out-dir results/

mod arpeggiator;
mod beeper;
mod benchmark;
mod cpu_load;
mod mml;
mod percussion;
mod piezo_model;
mod renderer;
mod sequence;
mod spectral;
mod via;
mod virtual_channel;

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};

use arpeggiator::ArpMode;
use piezo_model::DriveMode;
use renderer::{RenderConfig, Renderer};
use sequence::{Sequence, YamlSong};

// ─────────────────────────────────────────────────────────
// CLI 定義
// ─────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "lvb-sim",
    about = "Lumen VIA Beeper Simulator v0.1\nW65C22S VIA 2ch beeper + 疑似 4ch アルペジオ シミュレータ",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// 入力ファイル (.yaml / .mmml)
    input: Option<PathBuf>,

    /// 出力 WAV ファイル
    #[arg(long, short)]
    out: Option<PathBuf>,

    /// CPU クロック周波数 [Hz]
    #[arg(long, default_value_t = 1_000_000)]
    cpu: u32,

    /// サンプルレート [Hz]
    #[arg(long, default_value_t = 48_000)]
    sample_rate: u32,

    /// アルペジオ更新レート [Hz]
    #[arg(long, default_value_t = 240.0)]
    arp_rate: f32,

    /// アルペジオモード: Off / Pseudo3 / Pseudo4 / BassLock / MelodyLock
    #[arg(long, default_value = "BassLock")]
    mode: String,

    /// 圧電サウンダモデル: PS1720P02 (現状固定)
    #[arg(long, default_value = "PS1720P02")]
    piezo: String,

    /// 駆動モード: 3v3-btl / 3v3-single / 5v-btl / 5v-single
    #[arg(long, default_value = "3v3-btl")]
    drive: String,

    /// 圧電モデルを無効にする
    #[arg(long)]
    no_piezo: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// 複数設定でベンチマーク比較を実行
    Benchmark {
        /// 入力ファイル
        input: PathBuf,

        /// 単一の出力先 WAV (--compare 未指定時)
        #[arg(long)]
        out: Option<PathBuf>,

        /// 比較するモードをカンマ区切りで指定 (例: basslock,pseudo4)
        #[arg(long)]
        compare: Option<String>,

        /// 比較するアルペジオレートをカンマ区切りで指定 (例: 120,240,480)
        #[arg(long)]
        arp_rate: Option<String>,

        /// 圧電モデル (現状 PS1720P02 固定)
        #[arg(long, default_value = "PS1720P02")]
        piezo: String,

        /// 駆動モード
        #[arg(long, default_value = "3v3-btl")]
        drive: String,

        /// CPU クロック周波数 [Hz]
        #[arg(long, default_value_t = 1_000_000)]
        cpu: u32,

        /// サンプルレート [Hz]
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,

        /// 出力ディレクトリ (複数ファイルを出力)
        #[arg(long)]
        out_dir: Option<PathBuf>,

        /// 圧電モデルを無効にする
        #[arg(long)]
        no_piezo: bool,
    },
}

// ─────────────────────────────────────────────────────────
// パーサ補助関数
// ─────────────────────────────────────────────────────────

fn parse_arp_mode(s: &str) -> ArpMode {
    ArpMode::from_str(s)
}

fn parse_drive_mode(s: &str) -> DriveMode {
    match s.to_lowercase().as_str() {
        "3v3-single" | "3v3se" | "3v3single" => DriveMode::V33SingleEnded,
        "3v3-btl" | "3v3btl" | "3v3" => DriveMode::V33Btl,
        "5v-single" | "5vse" | "5vsingle" => DriveMode::V5SingleEnded,
        "5v-btl" | "5vbtl" | "5v" => DriveMode::V5Btl,
        _ => DriveMode::V33Btl,
    }
}

/// ファイル種別を自動判定して Sequence に変換
fn load_sequence(path: &Path) -> Result<Sequence> {
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_str()
        .unwrap_or("")
        .to_lowercase();

    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("ファイルを読み込めません '{}': {}", path.display(), e))?;

    match ext.as_str() {
        "yaml" | "yml" => {
            let yaml: YamlSong = serde_yaml::from_str(&content)
                .map_err(|e| anyhow!("YAML 解析エラー: {}", e))?;
            Sequence::from_yaml(yaml)
        }
        "mmml" | "mml" => mml::parse_mmml_file(&content),
        _ => {
            // 拡張子不明: YAML として試み、失敗したら mmml として試みる
            if let Ok(yaml) = serde_yaml::from_str::<YamlSong>(&content) {
                Sequence::from_yaml(yaml)
            } else {
                mml::parse_mmml_file(&content)
            }
        }
    }
}

// ─────────────────────────────────────────────────────────
// メイン
// ─────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        // ─── benchmark サブコマンド ────────────────────────────
        Some(Commands::Benchmark {
            input,
            out,
            compare,
            arp_rate,
            piezo: _,
            drive,
            cpu,
            sample_rate,
            out_dir,
            no_piezo,
        }) => {
            let sequence = load_sequence(&input)?;

            let modes: Vec<ArpMode> = if let Some(s) = compare {
                s.split(',').map(|m| parse_arp_mode(m.trim())).collect()
            } else {
                vec![ArpMode::BassLock]
            };

            let rates: Vec<f32> = if let Some(s) = arp_rate {
                s.split(',')
                    .filter_map(|r| r.trim().parse::<f32>().ok())
                    .collect()
            } else {
                vec![240.0]
            };

            let drive_mode = parse_drive_mode(&drive);

            benchmark::run_benchmark(
                &input,
                &sequence,
                modes,
                rates,
                drive_mode,
                cpu,
                sample_rate,
                !no_piezo,
                out.as_deref(),
                out_dir.as_deref(),
            )?;
        }

        // ─── デフォルト: 単一ファイルのレンダリング ───────────
        None => {
            let input = cli.input.ok_or_else(|| {
                anyhow!("入力ファイルを指定してください\n使用法: lvb-sim <input.yaml|input.mmml> --out output.wav")
            })?;

            println!("Lumen VIA Beeper Simulator v0.1");
            println!("  入力: {}", input.display());

            let sequence = load_sequence(&input)?;

            if let Some(ref title) = sequence.title {
                println!("  タイトル: {}", title);
            }
            println!("  テンポ: {:.1} BPM", sequence.source_tempo_bpm);
            println!("  イベント数: {}", sequence.events.len());
            println!("  推定再生時間: {:.2} 秒", sequence.total_duration_secs);
            println!();

            let arp_mode = parse_arp_mode(&cli.mode);
            let drive_mode = parse_drive_mode(&cli.drive);

            println!("  アルペジオモード: {}", arp_mode.name());
            println!("  アルペジオレート: {} Hz", cli.arp_rate);
            println!("  CPU クロック: {} Hz", cli.cpu);
            println!("  圧電モデル: {}", if cli.no_piezo { "無効" } else { "有効 (PS1720P02)" });
            println!("  駆動モード: {}", drive_mode.name());
            println!();

            let config = RenderConfig {
                sample_rate: cli.sample_rate,
                cpu_clock_hz: cli.cpu,
                arp_rate_hz: cli.arp_rate,
                arp_mode,
                piezo_enabled: !cli.no_piezo,
                drive_mode,
            };

            print!("レンダリング中...");
            std::io::stdout().flush().ok();

            let renderer = Renderer::new(config.clone());
            let result = renderer.render(&sequence)?;

            println!(" 完了");
            println!();

            result.print_summary(&config);

            // スペクトル分析結果
            if let Some(ref s) = result.spectral {
                println!();
                s.print();
            }

            // WAV 書き出し
            let out_path = cli.out.unwrap_or_else(|| {
                let stem = input.file_stem().unwrap_or_default().to_string_lossy();
                PathBuf::from(format!("{}_output.wav", stem))
            });
            result.write_wav(&out_path, cli.sample_rate)?;
            println!("\n  → WAV: {}", out_path.display());

            // ログ CSV 書き出し
            if let Some(parent) = out_path.parent() {
                let stem = out_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let via_path = parent.join(format!("{}_via_log.csv", stem));
                let cpu_path = parent.join(format!("{}_cpu_load.csv", stem));
                result.write_via_log(&via_path)?;
                result.write_cpu_log(&cpu_path)?;
                println!("  → VIA ログ: {}", via_path.display());
                println!("  → CPU ログ: {}", cpu_path.display());
            }

            // CPU 負荷レポート
            println!();
            cpu_load::print_load_report(&result.cpu_load, config.arp_mode.name(), config.cpu_clock_hz);
        }
    }

    Ok(())
}

// stdout を flush するために必要
use std::io::Write as _;
