/// メインオーディオレンダラ
///
/// Sequence を受け取り、VIA beeper + アルペジエータ + 圧電モデルを通して
/// PCM サンプル列を生成する。

use std::path::Path;

use anyhow::Result;

use crate::arpeggiator::{ArpMode, Arpeggiator};
use crate::beeper::BeeperChannel;
use crate::cpu_load::{CpuLoad, CpuLoadConfig, CpuLoadEstimator, CpuLoadSample};
use crate::piezo_model::{DriveMode, PiezoModel};
use crate::sequence::{Sequence, SongEvent};
use crate::spectral::SpectralMetrics;
use crate::via::ViaEmulator;
use crate::virtual_channel::{VirtualChannels};

// ─────────────────────────────────────────────────────────
// 設定
// ─────────────────────────────────────────────────────────

/// レンダリング設定
#[derive(Debug, Clone)]
pub struct RenderConfig {
    /// サンプルレート [Hz] (例: 48000)
    pub sample_rate: u32,
    /// CPU クロック周波数 [Hz] (例: 1_000_000)
    pub cpu_clock_hz: u32,
    /// アルペジオ更新レート [Hz] (例: 240.0)
    pub arp_rate_hz: f32,
    /// アルペジオモード
    pub arp_mode: ArpMode,
    /// 圧電サウンダモデルを有効にするか
    pub piezo_enabled: bool,
    /// 駆動電圧モード
    pub drive_mode: DriveMode,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            cpu_clock_hz: 1_000_000,
            arp_rate_hz: 240.0,
            arp_mode: ArpMode::BassLock,
            piezo_enabled: true,
            drive_mode: DriveMode::V33Btl,
        }
    }
}

// ─────────────────────────────────────────────────────────
// 統計
// ─────────────────────────────────────────────────────────

/// レンダリング統計
#[derive(Debug, Default, Clone)]
pub struct RenderStats {
    /// 入力イベント総数
    pub total_events: u32,
    /// 実際に発音されたイベント数
    pub played_events: u32,
    /// VIA レジスタ書き込み総数
    pub via_write_count: u64,
    /// アルペジオ更新総数
    pub arp_update_count: u64,
    /// ミュージックティック総数
    pub music_tick_count: u64,
    /// 音符保存率 (played / total)
    pub note_preservation_rate: f32,
    /// 1秒あたりのアルペジオ切替回数
    pub arp_switch_rate: f32,
    /// 1秒あたりの VIA 書き込み回数
    pub via_write_rate: f32,
}

// ─────────────────────────────────────────────────────────
// レンダリング結果
// ─────────────────────────────────────────────────────────

/// レンダリング結果
pub struct RenderResult {
    /// f32 PCM サンプル [-1.0, 1.0] (モノラル)
    pub samples: Vec<f32>,
    /// VIA レジスタログ
    pub via: ViaEmulator,
    /// CPU 負荷ログ (1 秒刻み)
    pub cpu_log: Vec<CpuLoadSample>,
    /// 計算済み CPU 負荷
    pub cpu_load: CpuLoad,
    /// 統計
    pub stats: RenderStats,
    /// 実際の再生時間 [秒]
    pub duration_seconds: f32,
    /// スペクトル分析結果 (圧電モデル有効時のみ Some)
    pub spectral: Option<SpectralMetrics>,
}

impl RenderResult {
    /// WAV ファイルとして書き出す (44.1kHz or 48kHz 16bit PCM モノラル)
    pub fn write_wav(&self, path: &Path, sample_rate: u32) -> Result<()> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec)?;
        for &s in &self.samples {
            let clamped = s.clamp(-1.0, 1.0);
            let i16_val = (clamped * i16::MAX as f32) as i16;
            writer.write_sample(i16_val)?;
        }
        writer.finalize()?;
        Ok(())
    }

    /// VIA レジスタログを CSV として書き出す
    pub fn write_via_log(&self, path: &Path) -> Result<()> {
        self.via.write_log_csv(path)
    }

    /// CPU 負荷ログを CSV として書き出す
    pub fn write_cpu_log(&self, path: &Path) -> Result<()> {
        crate::cpu_load::write_cpu_log_csv(&self.cpu_log, path)
    }

    /// 結果の概要を標準出力に表示
    pub fn print_summary(&self, config: &RenderConfig) {
        println!("  再生時間:     {:.2} 秒", self.duration_seconds);
        println!("  サンプル数:   {}", self.samples.len());
        println!("  イベント総数: {}", self.stats.total_events);
        println!("  音符保存率:   {:.1}%", self.stats.note_preservation_rate * 100.0);
        println!("  VIA書き込み:  {} 回 ({:.0}/秒)", self.stats.via_write_count, self.stats.via_write_rate);
        println!("  アルペジオ更新: {} 回", self.stats.arp_update_count);
        println!("  CPU負荷推定:  {:.1}% (最小) / {:.1}% (現実的) @ {}MHz",
            self.cpu_load.percent,
            self.cpu_load.realistic_percent,
            config.cpu_clock_hz / 1_000_000
        );
    }
}

// ─────────────────────────────────────────────────────────
// レンダラ本体
// ─────────────────────────────────────────────────────────

/// メインレンダラ
pub struct Renderer {
    config: RenderConfig,
}

impl Renderer {
    pub fn new(config: RenderConfig) -> Self {
        Self { config }
    }

    /// シーケンスをレンダリングして RenderResult を返す
    pub fn render(&self, sequence: &Sequence) -> Result<RenderResult> {
        let sr = self.config.sample_rate;
        let total_samples = (sequence.total_duration_secs * sr as f32).ceil() as usize;
        let mut samples = vec![0.0f32; total_samples];

        // 論理チャンネル
        let mut vchs = VirtualChannels::new();

        // 物理チャンネル
        let mut cha = BeeperChannel::new();
        let mut chb = BeeperChannel::new();

        // アルペジエータ
        let mut arp = Arpeggiator::new(self.config.arp_mode.clone(), self.config.arp_rate_hz);

        // VIA エミュレータ
        let mut via = ViaEmulator::new(self.config.cpu_clock_hz);

        // 圧電モデル
        let mut piezo = PiezoModel::new(self.config.drive_mode, sr);
        piezo.enabled = self.config.piezo_enabled;

        // CPU 負荷推定器
        let mut cpu_est = CpuLoadEstimator::new(CpuLoadConfig {
            cpu_clock_hz: self.config.cpu_clock_hz,
            ..Default::default()
        });
        cpu_est.duration_seconds = sequence.total_duration_secs;

        // CPU ログ (1 秒ごと)
        let mut cpu_log: Vec<CpuLoadSample> = Vec::new();
        let mut last_cpu_log_time = 0.0f32;

        // イベントキュー (既にソート済み)
        let events = &sequence.events;
        let mut event_idx = 0;

        // アルペジオタイミング (サンプル単位のアキュムレータ)
        let samples_per_arp = sr as f32 / self.config.arp_rate_hz;
        let mut arp_accum = 0.0f32;

        // 統計
        let total_events = events.len() as u32;
        let mut played_events = 0u32;
        let mut via_write_before;

        // ─── メインレンダリングループ ───────────────────────

        for i in 0..total_samples {
            let t = i as f32 / sr as f32;
            let t_us = (t * 1_000_000.0) as u64;

            // ─ 1. VCH ゲートを時刻に基づいて更新 ────────────
            vchs.update_gates(t);

            // ─ 2. 音符イベントを処理 ──────────────────────────
            while event_idx < events.len() && events[event_idx].time_secs <= t {
                let ev: &SongEvent = &events[event_idx];
                let vch = vchs.get_mut(ev.vchannel);
                vch.note_on(ev.frequency_hz, ev.volume, ev.gate_close_secs);
                played_events += 1;
                cpu_est.record_music_tick();
                event_idx += 1;
            }

            // ─ 3. アルペジオティック ──────────────────────────
            arp_accum += 1.0;
            if arp_accum >= samples_per_arp {
                arp_accum -= samples_per_arp;

                let assignment = arp.assign(&vchs.channels);
                arp.advance();

                via_write_before = via.log.len();

                // CHA 更新
                match assignment.cha_idx {
                    Some(idx) => {
                        let vch = &vchs.channels[idx];
                        if vch.is_active() {
                            cha.set_state(vch.frequency_hz, vch.volume, true);
                            via.set_channel_a(vch.frequency_hz, t_us);
                        } else {
                            cha.silence();
                        }
                    }
                    None => cha.silence(),
                }

                // CHB 更新
                match assignment.chb_idx {
                    Some(idx) => {
                        let vch = &vchs.channels[idx];
                        if vch.is_active() {
                            chb.set_state(vch.frequency_hz, vch.volume, true);
                            via.set_channel_b(vch.frequency_hz, t_us);
                        } else {
                            chb.silence();
                        }
                    }
                    None => chb.silence(),
                }

                let new_writes = via.log.len() - via_write_before;
                for _ in 0..new_writes {
                    cpu_est.record_via_write();
                }
                cpu_est.record_arp_update();
            }

            // ─ 4. PCM サンプル生成 ────────────────────────────
            let a = cha.generate_sample(sr as f32);
            let b = chb.generate_sample(sr as f32);
            samples[i] = (a + b) * 0.5;

            // ─ 5. CPU ログを 1 秒ごとに記録 ──────────────────
            if t - last_cpu_log_time >= 1.0 {
                let partial = cpu_est.compute();
                cpu_log.push(CpuLoadSample {
                    time_seconds: t,
                    load_percent: partial.percent,
                    via_writes_per_sec: partial.via_writes_per_sec,
                    arp_updates_per_sec: partial.arp_updates_per_sec,
                });
                last_cpu_log_time = t;
            }
        }

        // ─── 5. 圧電フィルタ適用 + スペクトル分析 ─────────────
        // 圧電有効時: フィルタ前のサンプルを保存して差分を計測
        let spectral = if self.config.piezo_enabled {
            let raw_samples = samples.clone();
            piezo.process_buffer(&mut samples);
            Some(crate::spectral::analyze(&raw_samples, &samples, sr))
        } else {
            piezo.process_buffer(&mut samples);
            None
        };

        // ─── 6. 統計計算 ──────────────────────────────────────
        let duration_seconds = total_samples as f32 / sr as f32;
        let final_cpu = cpu_est.compute();

        let stats = RenderStats {
            total_events,
            played_events,
            via_write_count: cpu_est.total_via_writes,
            arp_update_count: cpu_est.total_arp_updates,
            music_tick_count: cpu_est.total_music_ticks,
            note_preservation_rate: if total_events > 0 {
                played_events as f32 / total_events as f32
            } else {
                1.0
            },
            arp_switch_rate: cpu_est.total_arp_updates as f32 / duration_seconds,
            via_write_rate: cpu_est.total_via_writes as f32 / duration_seconds,
        };

        Ok(RenderResult {
            samples,
            via,
            cpu_log,
            cpu_load: final_cpu,
            stats,
            duration_seconds,
            spectral,
        })
    }
}

// ─────────────────────────────────────────────────────────
// 便利関数
// ─────────────────────────────────────────────────────────

/// f32 サンプル列を 16bit WAV として保存する
pub fn write_wav_file(samples: &[f32], sample_rate: u32, path: &Path) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        writer.write_sample((clamped * i16::MAX as f32) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}
