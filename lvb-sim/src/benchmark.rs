/// ベンチマークランナー
///
/// 同一の入力ファイルに対して複数の設定でレンダリングを行い、
/// WAV・CSV・サマリー MD を出力する。
///
/// 要件定義 24.6-24.8 参照。

use std::path::{Path, PathBuf};
use std::io::Write as IoWrite;

use anyhow::Result;

use crate::arpeggiator::ArpMode;
use crate::piezo_model::DriveMode;
use crate::renderer::{RenderConfig, Renderer, RenderStats};
use crate::sequence::Sequence;
use crate::cpu_load::CpuLoad;
use crate::spectral::SpectralMetrics;

/// ベンチマーク 1 ケースの結果
struct BenchCase {
    mode_name: String,
    arp_rate_hz: f32,
    wav_path: PathBuf,
    stats: RenderStats,
    cpu_load: CpuLoad,
    duration_secs: f32,
    /// 圧電スペクトル分析結果 (piezo 有効時のみ Some)
    spectral: Option<SpectralMetrics>,
}

/// ベンチマーク実行
///
/// `modes` × `rates` の全組み合わせをレンダリングする。
pub fn run_benchmark(
    input_path: &Path,
    sequence: &Sequence,
    modes: Vec<ArpMode>,
    rates: Vec<f32>,
    drive_mode: DriveMode,
    cpu_clock_hz: u32,
    sample_rate: u32,
    piezo_enabled: bool,
    single_out: Option<&Path>,
    out_dir: Option<&Path>,
) -> Result<()> {
    // 出力先ディレクトリ
    let out_dir = if let Some(d) = out_dir {
        std::fs::create_dir_all(d)?;
        d.to_path_buf()
    } else {
        PathBuf::from(".")
    };

    let input_stem = input_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    println!("─── ベンチマーク開始: {} ───", input_path.display());
    println!("  モード: {:?}", modes.iter().map(|m| m.name()).collect::<Vec<_>>());
    println!("  レート: {:?} Hz", rates);
    println!("  出力先: {}", out_dir.display());
    println!();

    let mut cases: Vec<BenchCase> = Vec::new();

    for mode in &modes {
        for &rate in &rates {
            let config = RenderConfig {
                sample_rate,
                cpu_clock_hz,
                arp_rate_hz: rate,
                arp_mode: mode.clone(),
                piezo_enabled,
                drive_mode,
            };

            let label = format!("{}_{:.0}hz", mode.name().to_lowercase().replace(' ', "_"), rate);
            let wav_name = format!("{}_{}.wav", input_stem, label);
            let wav_path = out_dir.join(&wav_name);

            print!("  レンダリング: {} ... ", label);
            std::io::stdout().flush().ok();

            let renderer = Renderer::new(config.clone());
            let result = renderer.render(sequence)?;

            result.write_wav(&wav_path, sample_rate)?;

            // 最初のケースで VIA ログと CPU ログを書き出す
            if cases.is_empty() {
                let via_path = out_dir.join(format!("{}_{}_via_log.csv", input_stem, label));
                let cpu_path = out_dir.join(format!("{}_{}_cpu_load.csv", input_stem, label));
                result.write_via_log(&via_path)?;
                result.write_cpu_log(&cpu_path)?;
            }

            // 進捗表示 (スペクトル情報も簡易表示)
            let spectral_hint = match &result.spectral {
                Some(s) => format!(
                    ", 低音減衰={:+.0}dB, 実用帯域={:+.0}dB",
                    s.low_band_loss_db, s.piezo_band_energy_db
                ),
                None => String::new(),
            };
            println!(
                "完了 ({:.2}s, CPU {:.1}%→{:.1}%, 音符保存率 {:.0}%{})",
                result.duration_seconds,
                result.cpu_load.percent,
                result.cpu_load.realistic_percent,
                result.stats.note_preservation_rate * 100.0,
                spectral_hint,
            );

            cases.push(BenchCase {
                mode_name: format!("{} @ {:.0}Hz", mode.name(), rate),
                arp_rate_hz: rate,
                wav_path,
                stats: result.stats,
                cpu_load: result.cpu_load,
                duration_secs: result.duration_seconds,
                spectral: result.spectral,
            });
        }
    }

    // single_out が指定されている場合は最初のケースをコピー
    if let (Some(out), Some(first)) = (single_out, cases.first()) {
        std::fs::copy(&first.wav_path, out)?;
        println!("\n  出力: {}", out.display());
    }

    // サマリー MD の生成
    let summary_path = out_dir.join(format!("{}_summary.md", input_stem));
    write_summary_md(&summary_path, input_path, &cases, sequence)?;
    println!("\n  サマリー: {}", summary_path.display());

    // 比較 CSV の生成
    let csv_path = out_dir.join(format!("{}_analysis.csv", input_stem));
    write_analysis_csv(&csv_path, &cases)?;
    println!("  分析 CSV: {}", csv_path.display());

    Ok(())
}

/// サマリー Markdown を生成
fn write_summary_md(
    path: &Path,
    input_path: &Path,
    cases: &[BenchCase],
    sequence: &Sequence,
) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "# LVB-Sim ベンチマーク結果")?;
    writeln!(f)?;
    writeln!(f, "## 入力ファイル")?;
    writeln!(f)?;
    writeln!(f, "| 項目 | 値 |")?;
    writeln!(f, "|------|-----|")?;
    writeln!(f, "| ファイル | {} |", input_path.display())?;
    if let Some(ref title) = sequence.title {
        writeln!(f, "| タイトル | {} |", title)?;
    }
    writeln!(f, "| テンポ | {:.1} BPM |", sequence.source_tempo_bpm)?;
    writeln!(f, "| イベント数 | {} |", sequence.events.len())?;
    writeln!(f, "| 再生時間 | {:.2} 秒 |", sequence.total_duration_secs)?;
    writeln!(f)?;
    writeln!(f, "## 設定別結果")?;
    writeln!(f)?;
    // 圧電メトリクスがあるか確認
    let has_spectral = cases.iter().any(|c| c.spectral.is_some());

    if has_spectral {
        writeln!(
            f,
            "| モード | レート | CPU負荷 | 音符保存率 | 低音減衰 | 実用帯域 | 帯域占有率 | 全体ゲイン |"
        )?;
        writeln!(
            f,
            "|--------|--------|---------|------------|----------|----------|------------|------------|"
        )?;
        for c in cases {
            let (low_db, band_db, ratio_pct, gain_db) = match &c.spectral {
                Some(s) => (
                    format!("{:+.1}dB", s.low_band_loss_db),
                    format!("{:+.1}dB", s.piezo_band_energy_db),
                    format!("{:.1}%", s.useful_band_ratio_pct),
                    format!("{:+.1}dB", s.overall_gain_db),
                ),
                None => ("N/A".into(), "N/A".into(), "N/A".into(), "N/A".into()),
            };
            writeln!(
                f,
                "| {} | {:.0}Hz | {:.1}%→{:.1}% | {:.0}% | {} | {} | {} | {} |",
                c.mode_name,
                c.arp_rate_hz,
                c.cpu_load.percent,
                c.cpu_load.realistic_percent,
                c.stats.note_preservation_rate * 100.0,
                low_db,
                band_db,
                ratio_pct,
                gain_db,
            )?;
        }
    } else {
        writeln!(
            f,
            "| モード | レート | 再生時間 | CPU負荷 | 音符保存率 | VIA書込/秒 | アルペジオ更新/秒 |"
        )?;
        writeln!(
            f,
            "|--------|--------|----------|---------|------------|------------|------------------|"
        )?;
        for c in cases {
            writeln!(
                f,
                "| {} | {:.0}Hz | {:.2}s | {:.1}% | {:.0}% | {:.0} | {:.0} |",
                c.mode_name,
                c.arp_rate_hz,
                c.duration_secs,
                c.cpu_load.percent,
                c.stats.note_preservation_rate * 100.0,
                c.stats.via_write_rate,
                c.stats.arp_switch_rate,
            )?;
        }
    }
    writeln!(f)?;
    writeln!(f, "## 評価指標")?;
    writeln!(f)?;
    writeln!(f, "| 指標 | 説明 |")?;
    writeln!(f, "|------|------|")?;
    writeln!(f, "| `note_preservation_rate` | 元データの音符を鳴らせた割合 |")?;
    writeln!(f, "| `estimated_cpu_load` | 1MHz 65C02 想定 CPU 負荷率 |")?;
    writeln!(f, "| `via_write_rate` | 1秒あたりの VIA レジスタ更新回数 |")?;
    writeln!(f, "| `arp_switch_rate` | 1秒あたりの音程切替回数 |")?;
    if has_spectral {
        writeln!(f, "| `low_band_loss` | <500Hz 低音成分の圧電フィルタ減衰量 [dB] |")?;
        writeln!(f, "| `piezo_band_energy` | 1kHz-4kHz 実用帯域エネルギー (生信号全帯域比) [dB] |")?;
        writeln!(f, "| `useful_band_ratio` | 出力に占める実用帯域 (1-4kHz) の割合 |")?;
        writeln!(f, "| `overall_gain` | 圧電フィルタによる全体音量変化 [dB] |")?;
    }
    writeln!(f)?;
    writeln!(f, "---")?;
    writeln!(f, "*Generated by lvb-sim v0.1*")?;
    Ok(())
}

/// 分析 CSV を生成
fn write_analysis_csv(path: &Path, cases: &[BenchCase]) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    writeln!(
        f,
        "mode,arp_rate_hz,duration_s,\
         cpu_load_min_pct,cpu_load_realistic_pct,\
         note_preservation_rate,via_write_rate,arp_switch_rate,\
         low_band_loss_db,piezo_band_energy_db,useful_band_ratio_pct,overall_gain_db,\
         raw_rms,piezo_rms,wav_file"
    )?;
    for c in cases {
        let (low_db, band_db, ratio, gain, raw_rms, piezo_rms) = match &c.spectral {
            Some(s) => (
                format!("{:.2}", s.low_band_loss_db),
                format!("{:.2}", s.piezo_band_energy_db),
                format!("{:.2}", s.useful_band_ratio_pct),
                format!("{:.2}", s.overall_gain_db),
                format!("{:.6}", s.raw_rms),
                format!("{:.6}", s.piezo_rms),
            ),
            None => ("".into(), "".into(), "".into(), "".into(), "".into(), "".into()),
        };
        writeln!(
            f,
            "{},{:.0},{:.3},{:.2},{:.2},{:.4},{:.1},{:.1},{},{},{},{},{},{},{}",
            c.mode_name,
            c.arp_rate_hz,
            c.duration_secs,
            c.cpu_load.percent,
            c.cpu_load.realistic_percent,
            c.stats.note_preservation_rate,
            c.stats.via_write_rate,
            c.stats.arp_switch_rate,
            low_db, band_db, ratio, gain,
            raw_rms, piezo_rms,
            c.wav_path.file_name().unwrap_or_default().to_string_lossy()
        )?;
    }
    Ok(())
}
