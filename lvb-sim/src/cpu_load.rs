/// CPU 負荷推定モジュール
///
/// 1MHz / 500kHz の 65C02 でこのシーケンスを実行した場合の
/// CPU 消費サイクルを推定する。

/// CPU 負荷推定の設定パラメータ
///
/// ## 計上する処理カテゴリ
///
/// **明示的に計上する処理:**
/// - VIA レジスタ書き込み (`cycles_per_via_write`)
/// - アルペジオスロット選択・割当 (`cycles_per_arp_update`)
/// - ノートデータ読み出し・解釈 (`cycles_per_music_tick`)
/// - 割り込み入口/復帰 (`cycles_per_irq_overhead`)
/// - 全 VCH のノート長カウンタ管理 (`cycles_per_note_length_mgmt`)
///
/// **安全係数で吸収する処理:**
/// - ROM フェッチのウェイト、アドレスデコード遅延
/// - バンクスイッチ、ゼロページアクセス差分
/// - その他未計上のハウスキーピング
/// → `overhead_factor` (デフォルト 1.5) を最小推定値に乗じて「現実的推定値」を算出
#[derive(Debug, Clone)]
pub struct CpuLoadConfig {
    /// CPU クロック周波数 [Hz]
    pub cpu_clock_hz: u32,
    /// 1 ミュージックティックあたりのサイクル数
    ///
    /// 次のノートイベント読み出し・ピッチ計算・VCH 状態更新
    /// LDA/CMP/BEQ/JSR 等 ≈ 80 サイクル
    pub cycles_per_music_tick: u32,
    /// 1 アルペジオ更新あたりのサイクル数 (スロット選択のみ)
    ///
    /// 4VCH の優先度比較・アクティブ判定・割当決定
    /// ≈ 60 サイクル (ノート長管理・IRQ は別項)
    pub cycles_per_arp_update: u32,
    /// 1 VIA レジスタ書き込みあたりのサイクル数
    ///
    /// LDA #imm (2) + STA abs (4) × 2 レジスタ = 12 サイクル
    pub cycles_per_via_write: u32,
    /// 1 効果音トリガあたりのサイクル数
    pub cycles_per_effect_trigger: u32,

    // ─── 実機固有オーバーヘッド (現行モデルで未計上だった項目) ───

    /// IRQ 割り込み入口/復帰オーバーヘッド (アルペジオ更新 1 回あたり)
    ///
    /// W65C02S の IRQ ハードウェアシーケンス (7 サイクル) +
    /// ISR 内レジスタ保存 PHA/PHX/PHY (9) + 復帰 PLA/PLX/PLY/RTI (18)
    /// = 約 34 サイクル
    pub cycles_per_irq_overhead: u32,
    /// ノート長カウンタ管理 (アルペジオ更新 1 回あたり・全 VCH 合計)
    ///
    /// 各 VCH: LDA cnt / DEC / STA / BEQ ≈ 14 サイクル × 4 VCH = 56〜80 サイクル
    pub cycles_per_note_length_mgmt: u32,
    /// 安全係数 — 最小推定値に乗じて「現実的推定値」を算出
    ///
    /// ROM ウェイト・アドレスデコード・未計上ハウスキーピング等を吸収する。
    /// 1.5 ≈ 約 50% の余裕 (実測値との一般的な差異範囲)
    pub overhead_factor: f32,
}

impl Default for CpuLoadConfig {
    fn default() -> Self {
        Self {
            cpu_clock_hz: 1_000_000,
            cycles_per_music_tick: 80,
            cycles_per_arp_update: 60,
            cycles_per_via_write: 12,
            cycles_per_effect_trigger: 50,
            // 実機オーバーヘッド
            cycles_per_irq_overhead: 34,    // IRQ 入口/復帰
            cycles_per_note_length_mgmt: 80, // 4 VCH × 20 サイクル
            overhead_factor: 1.5,
        }
    }
}

/// CPU 負荷推定器
#[derive(Debug, Default)]
pub struct CpuLoadEstimator {
    pub config: CpuLoadConfig,
    pub total_via_writes: u64,
    pub total_arp_updates: u64,
    pub total_music_ticks: u64,
    pub total_effect_triggers: u64,
    pub duration_seconds: f32,
}

/// CPU 負荷計算結果
#[derive(Debug, Clone)]
pub struct CpuLoad {
    /// 最小推定負荷率 [%] (明示的に計上した処理のみ)
    pub percent: f32,
    /// 現実的推定負荷率 [%] (IRQ・ノート長管理・安全係数込み)
    pub realistic_percent: f32,
    /// 1秒あたりの VIA レジスタ書き込み回数
    pub via_writes_per_sec: f32,
    /// 1秒あたりのアルペジオ更新回数
    pub arp_updates_per_sec: f32,
    /// 1秒あたりのミュージックティック数
    pub music_ticks_per_sec: f32,
    /// 1秒あたりの消費サイクル数 (最小推定)
    pub cycles_per_sec: f32,
}

impl CpuLoadEstimator {
    pub fn new(config: CpuLoadConfig) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }

    pub fn record_via_write(&mut self) {
        self.total_via_writes += 1;
    }

    pub fn record_arp_update(&mut self) {
        self.total_arp_updates += 1;
    }

    pub fn record_music_tick(&mut self) {
        self.total_music_ticks += 1;
    }

    pub fn record_effect_trigger(&mut self) {
        self.total_effect_triggers += 1;
    }

    /// 負荷率を計算する
    pub fn compute(&self) -> CpuLoad {
        let dur = self.duration_seconds.max(1e-6);

        let via_per_sec    = self.total_via_writes      as f32 / dur;
        let arp_per_sec    = self.total_arp_updates     as f32 / dur;
        let music_per_sec  = self.total_music_ticks     as f32 / dur;
        let effect_per_sec = self.total_effect_triggers as f32 / dur;

        // ── 最小推定: 現行モデルで明示的に計上する処理 ──────────
        let cycles_min = via_per_sec   * self.config.cycles_per_via_write    as f32
            + arp_per_sec              * self.config.cycles_per_arp_update   as f32
            + music_per_sec            * self.config.cycles_per_music_tick   as f32
            + effect_per_sec           * self.config.cycles_per_effect_trigger as f32;

        // ── 追加オーバーヘッド: 実機固有だが現行モデルで未計上 ───
        //   IRQ 入口/復帰: アルペジオ割り込み 1 回につき固定コスト
        let cycles_irq  = arp_per_sec  * self.config.cycles_per_irq_overhead       as f32;
        //   ノート長管理: アルペジオ更新ごとに全 VCH のカウンタを操作
        let cycles_note = arp_per_sec  * self.config.cycles_per_note_length_mgmt   as f32;

        let cycles_with_overhead = cycles_min + cycles_irq + cycles_note;

        // ── 現実的推定: 安全係数を掛けて雑費を吸収 ─────────────
        let cycles_realistic = cycles_with_overhead * self.config.overhead_factor;

        let hz = self.config.cpu_clock_hz as f32;
        let percent           = (cycles_min       / hz) * 100.0;
        let realistic_percent = (cycles_realistic  / hz) * 100.0;

        CpuLoad {
            percent,
            realistic_percent,
            via_writes_per_sec: via_per_sec,
            arp_updates_per_sec: arp_per_sec,
            music_ticks_per_sec: music_per_sec,
            cycles_per_sec: cycles_min,
        }
    }
}

/// CPU 負荷ログエントリ (時系列)
#[derive(Debug, Clone)]
pub struct CpuLoadSample {
    pub time_seconds: f32,
    pub load_percent: f32,
    pub via_writes_per_sec: f32,
    pub arp_updates_per_sec: f32,
}

/// CPU 負荷ログを CSV 形式で書き出す
pub fn write_cpu_log_csv(
    log: &[CpuLoadSample],
    path: &std::path::Path,
) -> anyhow::Result<()> {
    use std::io::Write as IoWrite;
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "time_s,load_percent,via_writes_per_s,arp_updates_per_s")?;
    for entry in log {
        writeln!(
            f,
            "{:.3},{:.2},{:.1},{:.1}",
            entry.time_seconds,
            entry.load_percent,
            entry.via_writes_per_sec,
            entry.arp_updates_per_sec
        )?;
    }
    Ok(())
}

/// 目標負荷率との比較表示
pub fn print_load_report(load: &CpuLoad, arp_mode_name: &str, cpu_clock_hz: u32) {
    let mhz = cpu_clock_hz as f32 / 1_000_000.0;
    println!("  CPU 負荷推定 (@ {:.1}MHz, {})", mhz, arp_mode_name);
    println!("    VIA書き込み/秒:        {:.0}", load.via_writes_per_sec);
    println!("    アルペジオ更新/秒:     {:.0}", load.arp_updates_per_sec);
    println!("    ミュージックティック/秒: {:.0}", load.music_ticks_per_sec);
    println!();
    println!("    最小推定  (VIA書込+Arp選択+音符読出のみ): {:.1}%", load.percent);
    println!("    現実的推定 (+IRQ/ノート長管理×安全係数):  {:.1}%", load.realistic_percent);

    // 要件定義 10.5 との比較 (現実的推定値で判定)
    let target = match arp_mode_name {
        s if s.contains("BassLock") || s.contains("MelodyLock") => 20.0,
        s if s.contains("Pseudo4") || s.contains("Pseudo3")     => 30.0,
        _ => 25.0,
    };
    let status = if load.realistic_percent <= target { "✓ OK" } else { "⚠ 超過" };
    println!("    目標 {:.0}% 以下: {} (現実的推定 {:.1}%)", target, status, load.realistic_percent);
}
