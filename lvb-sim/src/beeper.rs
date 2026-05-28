/// 物理チャンネル — VIA Timer による矩形波生成器
///
/// 実機では VIA Timer がトグルする PB7/CB2 出力を模擬する。
/// 位相は周波数変更時にリセットしない（実機のタイマーと同様）。

#[derive(Debug, Clone)]
pub struct BeeperChannel {
    /// 発振周波数 [Hz]
    pub frequency_hz: f32,
    /// 位相 [0.0, 1.0)
    phase: f32,
    /// デューティ比 (0.5 = 50% 矩形波)
    pub duty: f32,
    /// ゲート: true で発音、false で無音
    pub gate: bool,
    /// 音量 [0.0, 1.0]
    pub volume: f32,
}

impl BeeperChannel {
    pub fn new() -> Self {
        Self {
            frequency_hz: 440.0,
            phase: 0.0,
            duty: 0.5,
            gate: false,
            volume: 1.0,
        }
    }

    /// 周波数・音量・ゲートをまとめて更新
    pub fn set_state(&mut self, frequency_hz: f32, volume: f32, gate: bool) {
        self.frequency_hz = frequency_hz;
        self.volume = volume.clamp(0.0, 1.0);
        self.gate = gate;
    }

    /// ゲートを閉じて無音にする
    pub fn silence(&mut self) {
        self.gate = false;
    }

    /// 1 サンプル生成する
    ///
    /// 位相は常に進める（ゲートが閉じていても）。
    /// これにより、ゲート再開時の不連続を最小化する。
    pub fn generate_sample(&mut self, sample_rate: f32) -> f32 {
        let out = if self.gate && self.frequency_hz > 0.0 {
            if self.phase < self.duty {
                self.volume
            } else {
                -self.volume
            }
        } else {
            0.0
        };

        // 位相を進める
        if self.frequency_hz > 0.0 && sample_rate > 0.0 {
            self.phase += self.frequency_hz / sample_rate;
            while self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }

        out
    }

    /// 現在の位相をリセット（デバッグ用）
    pub fn reset_phase(&mut self) {
        self.phase = 0.0;
    }
}

impl Default for BeeperChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_square_wave_50pct() {
        let mut ch = BeeperChannel::new();
        ch.set_state(1000.0, 1.0, true);
        let sr = 48000.0f32;

        // 1000 Hz サイン波のサンプル数 = 48000/1000 = 48 サンプル/周期
        // 最初の半周期は +1.0、次の半周期は -1.0
        let samples: Vec<f32> = (0..48).map(|_| ch.generate_sample(sr)).collect();
        let positive_count = samples.iter().filter(|&&s| s > 0.5).count();
        let negative_count = samples.iter().filter(|&&s| s < -0.5).count();
        // 50% デューティなので各半々
        assert!((positive_count as i32 - 24).abs() <= 1);
        assert!((negative_count as i32 - 24).abs() <= 1);
    }

    #[test]
    fn test_gate_silences() {
        let mut ch = BeeperChannel::new();
        ch.set_state(440.0, 1.0, false);
        for _ in 0..100 {
            assert_eq!(ch.generate_sample(48000.0), 0.0);
        }
    }
}
