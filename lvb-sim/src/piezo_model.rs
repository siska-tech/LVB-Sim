/// 圧電サウンダ音響モデル
///
/// TDK PS1720P02 相当の圧電サウンダ特性を簡易的に再現する。
///
/// フィルタチェーン:
///   入力波形
///     → DC カット高域通過フィルタ (300-800 Hz)
///     → 共振ピーク (~2kHz, バンドパス EQ)
///     → 帯域制限低域通過フィルタ (8-12 kHz)
///     → ソフトクリッピング (オプション)
///     → 出力

/// 駆動電圧モード
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DriveMode {
    /// 3.3V シングルエンド駆動 (振幅小)
    V33SingleEnded,
    /// 3.3V BTL 駆動 (振幅中)
    V33Btl,
    /// 5V シングルエンド駆動 (振幅中)
    V5SingleEnded,
    /// 5V BTL 駆動 (振幅大)
    V5Btl,
}

impl DriveMode {
    pub fn amplitude_scale(self) -> f32 {
        match self {
            DriveMode::V33SingleEnded => 0.45,
            DriveMode::V33Btl => 0.75,
            DriveMode::V5SingleEnded => 0.65,
            DriveMode::V5Btl => 1.0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DriveMode::V33SingleEnded => "3.3V single-ended",
            DriveMode::V33Btl => "3.3V BTL",
            DriveMode::V5SingleEnded => "5V single-ended",
            DriveMode::V5Btl => "5V BTL",
        }
    }
}

/// Biquad フィルタ (Direct Form I)
///
/// 伝達関数: H(z) = (b0 + b1*z^-1 + b2*z^-2) / (1 + a1*z^-1 + a2*z^-2)
#[derive(Debug, Clone)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    /// 1 サンプル処理
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    /// 高域通過フィルタ (Butterworth 2次)
    fn highpass(cutoff_hz: f64, q: f64, sample_rate: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * cutoff_hz / sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 + cos_w0) / 2.0 / a0,
            b1: -(1.0 + cos_w0) / a0,
            b2: (1.0 + cos_w0) / 2.0 / a0,
            a1: -2.0 * cos_w0 / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0,
        }
    }

    /// 低域通過フィルタ (Butterworth 2次)
    fn lowpass(cutoff_hz: f64, q: f64, sample_rate: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * cutoff_hz / sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cos_w0) / 2.0 / a0,
            b1: (1.0 - cos_w0) / a0,
            b2: (1.0 - cos_w0) / 2.0 / a0,
            a1: -2.0 * cos_w0 / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0,
        }
    }

    /// ピーキング EQ フィルタ (共振周波数でブースト)
    fn peaking(center_hz: f64, q: f64, gain_db: f64, sample_rate: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * center_hz / sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let a_gain = 10.0_f64.powf(gain_db / 40.0);
        let cos_w0 = w0.cos();
        let a0 = 1.0 + alpha / a_gain;
        Self {
            b0: (1.0 + alpha * a_gain) / a0,
            b1: -2.0 * cos_w0 / a0,
            b2: (1.0 - alpha * a_gain) / a0,
            a1: -2.0 * cos_w0 / a0,
            a2: (1.0 - alpha / a_gain) / a0,
            x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0,
        }
    }
}

/// 圧電サウンダ音響モデル
pub struct PiezoModel {
    /// フィルタ有効フラグ
    pub enabled: bool,
    /// 駆動モード
    pub drive_mode: DriveMode,
    /// 高域通過フィルタ (DC カット + 低域減衰)
    hp: Biquad,
    /// 共振ピーク (~2kHz)
    peak: Biquad,
    /// 帯域制限低域通過フィルタ
    lp: Biquad,
    /// 駆動モードによる振幅スケール
    drive_gain: f32,
}

impl PiezoModel {
    /// TDK PS1720P02 相当のモデルを構築
    ///
    /// - 高域通過: 500Hz (DC カット・低域減衰)
    /// - 共振ピーク: 2kHz、+6dB、Q=2.0
    /// - 低域通過: 10kHz (高域制限)
    pub fn new(drive_mode: DriveMode, sample_rate: u32) -> Self {
        let sr = sample_rate as f64;
        Self {
            enabled: true,
            drive_mode,
            hp: Biquad::highpass(500.0, std::f64::consts::FRAC_1_SQRT_2, sr),
            // 共振ピーク: +6dB → +3dB (矩形波との組み合わせで過剰なブーストを緩和)
            peak: Biquad::peaking(2000.0, 2.0, 3.0, sr),
            // 帯域制限: 10kHz → 7kHz (5kHz 以上の倍音は音楽的意味が薄く耳への刺激になる)
            lp: Biquad::lowpass(7000.0, std::f64::consts::FRAC_1_SQRT_2, sr),
            drive_gain: drive_mode.amplitude_scale(),
        }
    }

    /// フィルタ状態をリセット
    pub fn reset(&mut self) {
        self.hp.reset();
        self.peak.reset();
        self.lp.reset();
    }

    /// 1 サンプルを処理する
    #[inline]
    pub fn process_sample(&mut self, x: f32) -> f32 {
        if !self.enabled {
            return x * self.drive_gain;
        }
        let x64 = x as f64;
        let y = self.hp.process(x64);
        let y = self.peak.process(y);
        let y = self.lp.process(y);
        // ソフトクリッピング (tanh で自然な飽和感)
        let clipped = (y * self.drive_gain as f64).tanh();
        clipped as f32
    }

    /// バッファ全体を処理する
    pub fn process_buffer(&mut self, buf: &mut [f32]) {
        for sample in buf.iter_mut() {
            *sample = self.process_sample(*sample);
        }
    }

    /// 設定の概要を表示
    pub fn describe(&self) -> String {
        format!(
            "PiezoModel [{}{}]",
            self.drive_mode.name(),
            if self.enabled { "" } else { ", disabled" }
        )
    }
}
