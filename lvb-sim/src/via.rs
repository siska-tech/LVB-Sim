/// W65C22S VIA エミュレータ
///
/// Timer 1 / Timer 2 による矩形波生成をシミュレートする。
///
/// 周波数計算式:
///   f = cpu_clock_hz / (2 * (latch + 2))
///   latch = cpu_clock_hz / (2 * f) - 2
///
/// 例: 1MHz クロック、440Hz:
///   latch = 1000000 / 880 - 2 ≈ 1134
///   実際の周波数 = 1000000 / (2 * 1136) ≈ 440.14 Hz

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViaReg {
    T1CL,
    T1CH,
    T2CL,
    T2CH,
}

impl ViaReg {
    pub fn name(self) -> &'static str {
        match self {
            ViaReg::T1CL => "T1CL",
            ViaReg::T1CH => "T1CH",
            ViaReg::T2CL => "T2CL",
            ViaReg::T2CH => "T2CH",
        }
    }
}

/// VIA レジスタ書き込みログエントリ
#[derive(Debug, Clone)]
pub struct ViaRegWrite {
    pub time_us: u64,
    pub reg: ViaReg,
    pub value: u8,
    pub description: String,
}

/// W65C22S VIA エミュレータ
#[derive(Debug)]
pub struct ViaEmulator {
    pub timer1_latch: u16,
    pub timer2_latch: u16,
    pub cpu_clock_hz: u32,
    pub log: Vec<ViaRegWrite>,
}

impl ViaEmulator {
    pub fn new(cpu_clock_hz: u32) -> Self {
        Self {
            timer1_latch: 0,
            timer2_latch: 0,
            cpu_clock_hz,
            log: Vec::new(),
        }
    }

    /// 周波数 → VIA Timer ラッチ値に変換
    pub fn freq_to_latch(freq_hz: f32, cpu_clock_hz: u32) -> u16 {
        if freq_hz <= 0.0 {
            return u16::MAX;
        }
        let latch = cpu_clock_hz as f32 / (2.0 * freq_hz) - 2.0;
        latch.round().clamp(0.0, u16::MAX as f32) as u16
    }

    /// VIA Timer ラッチ値 → 実際の周波数
    pub fn latch_to_freq(latch: u16, cpu_clock_hz: u32) -> f32 {
        cpu_clock_hz as f32 / (2.0 * (latch as f32 + 2.0))
    }

    /// CH-A (Timer 1 / PB7) の周波数を設定してレジスタログを記録
    pub fn set_channel_a(&mut self, freq_hz: f32, time_us: u64) {
        if freq_hz <= 0.0 {
            return;
        }
        let latch = Self::freq_to_latch(freq_hz, self.cpu_clock_hz);
        if latch == self.timer1_latch {
            return; // 変化なし
        }
        self.timer1_latch = latch;
        let lo = (latch & 0xFF) as u8;
        let hi = (latch >> 8) as u8;
        self.log.push(ViaRegWrite {
            time_us,
            reg: ViaReg::T1CL,
            value: lo,
            description: format!("CH-A {:.1}Hz low", freq_hz),
        });
        self.log.push(ViaRegWrite {
            time_us,
            reg: ViaReg::T1CH,
            value: hi,
            description: format!("CH-A {:.1}Hz high", freq_hz),
        });
    }

    /// CH-B (Timer 2 / CB2) の周波数を設定してレジスタログを記録
    pub fn set_channel_b(&mut self, freq_hz: f32, time_us: u64) {
        if freq_hz <= 0.0 {
            return;
        }
        let latch = Self::freq_to_latch(freq_hz, self.cpu_clock_hz);
        if latch == self.timer2_latch {
            return; // 変化なし
        }
        self.timer2_latch = latch;
        let lo = (latch & 0xFF) as u8;
        let hi = (latch >> 8) as u8;
        self.log.push(ViaRegWrite {
            time_us,
            reg: ViaReg::T2CL,
            value: lo,
            description: format!("CH-B {:.1}Hz low", freq_hz),
        });
        self.log.push(ViaRegWrite {
            time_us,
            reg: ViaReg::T2CH,
            value: hi,
            description: format!("CH-B {:.1}Hz high", freq_hz),
        });
    }

    /// VIA レジスタログを CSV 形式で書き出す
    pub fn write_log_csv(&self, path: &std::path::Path) -> anyhow::Result<()> {
        use std::io::Write as IoWrite;
        let mut f = std::fs::File::create(path)?;
        writeln!(f, "time_us,reg,value_hex,value_dec,description")?;
        for entry in &self.log {
            writeln!(
                f,
                "{},{},0x{:02X},{},{}",
                entry.time_us,
                entry.reg.name(),
                entry.value,
                entry.value,
                entry.description
            )?;
        }
        Ok(())
    }

    /// 現在の CH-A 実周波数
    pub fn channel_a_freq(&self) -> f32 {
        Self::latch_to_freq(self.timer1_latch, self.cpu_clock_hz)
    }

    /// 現在の CH-B 実周波数
    pub fn channel_b_freq(&self) -> f32 {
        Self::latch_to_freq(self.timer2_latch, self.cpu_clock_hz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freq_round_trip() {
        let cpu = 1_000_000u32;
        for &target in &[262.0f32, 440.0, 880.0, 1000.0, 2000.0] {
            let latch = ViaEmulator::freq_to_latch(target, cpu);
            let actual = ViaEmulator::latch_to_freq(latch, cpu);
            let error = (actual - target).abs() / target;
            assert!(error < 0.02, "freq {} Hz: error {:.2}%", target, error * 100.0);
        }
    }

    #[test]
    fn test_a440() {
        let cpu = 1_000_000u32;
        let latch = ViaEmulator::freq_to_latch(440.0, cpu);
        let actual = ViaEmulator::latch_to_freq(latch, cpu);
        // 440 Hz での誤差は 0.1 Hz 以内であること
        assert!((actual - 440.0).abs() < 1.0, "A440: got {:.2} Hz", actual);
    }
}
