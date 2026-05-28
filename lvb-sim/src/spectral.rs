/// スペクトル分析 — 圧電サウンダモデルの効果を定量評価
///
/// FFT 不使用。Biquad IIR フィルタで帯域ごとの RMS エネルギーを計算し、
/// 圧電フィルタによる低音減衰量・実用帯域エネルギーを数値化する。
///
/// 分析帯域:
///   低域   : <500Hz   (圧電サウンダでほぼ消える帯域)
///   実用帯域: 1kHz-4kHz (PS1720P02 の実力帯域)
///   全帯域  : 0〜Nyquist

// ─────────────────────────────────────────────────────────
// 分析結果
// ─────────────────────────────────────────────────────────

/// 1 レンダリングケースのスペクトル分析結果
#[derive(Debug, Clone)]
pub struct SpectralMetrics {
    /// 圧電フィルタ前の全帯域 RMS
    pub raw_rms: f32,
    /// 圧電フィルタ後の全帯域 RMS
    pub piezo_rms: f32,
    /// 圧電フィルタ前の低域 (<500Hz) RMS
    pub raw_low_rms: f32,
    /// 圧電フィルタ後の低域 (<500Hz) RMS
    pub piezo_low_rms: f32,
    /// 圧電フィルタ後の実用帯域 (1kHz-4kHz) RMS
    pub piezo_band_rms: f32,
    /// 低音成分の減衰量 [dB]  (負値 = 減衰; 要件 §24.4 `low_band_loss`)
    pub low_band_loss_db: f32,
    /// 実用帯域エネルギー [dB, 生信号全帯域比]  (要件 §24.4 `piezo_band_energy`)
    pub piezo_band_energy_db: f32,
    /// 実用帯域が占める割合 [%] = 1-4kHz / 全帯域 (圧電後)
    pub useful_band_ratio_pct: f32,
    /// 全体的な音量変化 [dB] (圧電フィルタによるゲイン)
    pub overall_gain_db: f32,
}

impl SpectralMetrics {
    /// 標準出力に概要を表示
    pub fn print(&self) {
        println!("  スペクトル分析 (圧電モデル効果):");
        println!("    低音減衰量  (<500Hz)   : {:+.1} dB", self.low_band_loss_db);
        println!(
            "    実用帯域エネルギー (1-4kHz): {:+.1} dB  [{:.1}% of output]",
            self.piezo_band_energy_db, self.useful_band_ratio_pct
        );
        println!("    全体ゲイン              : {:+.1} dB", self.overall_gain_db);
        println!(
            "    RMS: 生={:.4}  圧電後={:.4}",
            self.raw_rms, self.piezo_rms
        );
    }
}

// ─────────────────────────────────────────────────────────
// 内部: 簡易 Biquad (Direct Form I, f64)
// ─────────────────────────────────────────────────────────

struct Biquad {
    b0: f64, b1: f64, b2: f64,
    a1: f64, a2: f64,
    x1: f64, x2: f64,
    y1: f64, y2: f64,
}

impl Biquad {
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1; self.x1 = x;
        self.y2 = self.y1; self.y1 = y;
        y
    }

    /// Butterworth 2次 低域通過
    fn lowpass(fc: f64, q: f64, sr: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * fc / sr;
        let alpha = w0.sin() / (2.0 * q);
        let cw = w0.cos();
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cw) / 2.0 / a0,
            b1: (1.0 - cw) / a0,
            b2: (1.0 - cw) / 2.0 / a0,
            a1: -2.0 * cw / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0,
        }
    }

    /// Butterworth 2次 高域通過
    fn highpass(fc: f64, q: f64, sr: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * fc / sr;
        let alpha = w0.sin() / (2.0 * q);
        let cw = w0.cos();
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 + cw) / 2.0 / a0,
            b1: -(1.0 + cw) / a0,
            b2: (1.0 + cw) / 2.0 / a0,
            a1: -2.0 * cw / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0,
        }
    }
}

// ─────────────────────────────────────────────────────────
// ヘルパ関数
// ─────────────────────────────────────────────────────────

/// サンプル列の全帯域 RMS
fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() { return 0.0; }
    let sum_sq: f64 = samples.iter().map(|&x| (x as f64) * (x as f64)).sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

/// Biquad フィルタを適用したサンプル列の RMS
fn filtered_rms_1(samples: &[f32], mut f: Biquad) -> f32 {
    if samples.is_empty() { return 0.0; }
    let sum_sq: f64 = samples.iter()
        .map(|&x| { let y = f.process(x as f64); y * y })
        .sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

/// Biquad 2段カスケードを適用したサンプル列の RMS
fn filtered_rms_2(samples: &[f32], mut f1: Biquad, mut f2: Biquad) -> f32 {
    if samples.is_empty() { return 0.0; }
    let sum_sq: f64 = samples.iter()
        .map(|&x| {
            let y = f1.process(x as f64);
            let y = f2.process(y);
            y * y
        })
        .sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

/// RMS 比を dB に変換 (ゼロ保護付き)
fn ratio_to_db(ratio: f32) -> f32 {
    if ratio < 1e-10 { return -100.0; }
    20.0 * ratio.log10()
}

// ─────────────────────────────────────────────────────────
// 公開 API
// ─────────────────────────────────────────────────────────

/// スペクトル分析を実行する
///
/// # 引数
/// - `raw`       : 圧電フィルタ適用前のサンプル列
/// - `processed` : 圧電フィルタ適用後のサンプル列 (`raw` と同じ長さ)
/// - `sample_rate`: サンプルレート [Hz]
pub fn analyze(raw: &[f32], processed: &[f32], sample_rate: u32) -> SpectralMetrics {
    let sr = sample_rate as f64;
    let q_bw = std::f64::consts::FRAC_1_SQRT_2; // Butterworth Q (≈0.707)

    // ── 全帯域 RMS ────────────────────────────────────────
    let raw_rms   = rms(raw);
    let piezo_rms = rms(processed);

    // ── 低域 (<500Hz) RMS ─────────────────────────────────
    let raw_low_rms   = filtered_rms_1(raw,       Biquad::lowpass(500.0, q_bw, sr));
    let piezo_low_rms = filtered_rms_1(processed, Biquad::lowpass(500.0, q_bw, sr));

    // ── 実用帯域 (1kHz-4kHz) RMS — HP@1kHz → LP@4kHz ─────
    let piezo_band_rms = filtered_rms_2(
        processed,
        Biquad::highpass(1000.0, q_bw, sr),
        Biquad::lowpass(4000.0, q_bw, sr),
    );

    // ── dB 計算 ───────────────────────────────────────────

    // 低音減衰量: 圧電後の低域 vs 生の低域
    let low_band_loss_db = ratio_to_db(if raw_low_rms > 1e-10 {
        piezo_low_rms / raw_low_rms
    } else {
        1.0
    });

    // 実用帯域エネルギー: 圧電後の 1-4kHz vs 生の全帯域
    let piezo_band_energy_db = ratio_to_db(if raw_rms > 1e-10 {
        piezo_band_rms / raw_rms
    } else {
        0.0
    });

    // 実用帯域比率
    let useful_band_ratio_pct = if piezo_rms > 1e-10 {
        piezo_band_rms / piezo_rms * 100.0
    } else {
        0.0
    };

    // 全体ゲイン
    let overall_gain_db = ratio_to_db(if raw_rms > 1e-10 {
        piezo_rms / raw_rms
    } else {
        1.0
    });

    SpectralMetrics {
        raw_rms,
        piezo_rms,
        raw_low_rms,
        piezo_low_rms,
        piezo_band_rms,
        low_band_loss_db,
        piezo_band_energy_db,
        useful_band_ratio_pct,
        overall_gain_db,
    }
}
