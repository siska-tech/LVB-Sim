/// アルペジエータ — 論理チャンネルから物理チャンネルへの割当スケジューラ
///
/// 高速に物理チャンネルの周波数を切り替えることで、
/// 2ch から疑似的に 3〜4ch の多声表現を実現する。
///
/// 要件定義 6.2 のアルペジオパターン:
///   BassLock モード (推奨):
///     Tick 0: CH-A=VCH1, CH-B=VCH2
///     Tick 1: CH-A=VCH3, CH-B=VCH2
///     Tick 2: CH-A=VCH1, CH-B=VCH4
///     Tick 3: CH-A=VCH3, CH-B=VCH2

use crate::virtual_channel::VirtualChannel;

/// アルペジオモード
#[derive(Debug, Clone, PartialEq)]
pub enum ArpMode {
    /// 直接 2ch: VCH1→CHA, VCH2→CHB (アルペジオなし)
    Off,
    /// 疑似 3ch: CHA が VCH1/VCH3 を交互、CHB=VCH2
    Pseudo3,
    /// 疑似 4ch: CHA/CHB が VCH1〜4 を時分割
    Pseudo4,
    /// ベースロック: CHB=VCH2 固定、CHA で VCH1/VCH3/VCH4 を時分割
    BassLock,
    /// メロディロック: CHA=VCH1 固定、CHB で VCH2/VCH3/VCH4 を時分割
    MelodyLock,
}

impl ArpMode {
    pub fn name(&self) -> &'static str {
        match self {
            ArpMode::Off => "Off (direct2)",
            ArpMode::Pseudo3 => "Pseudo3",
            ArpMode::Pseudo4 => "Pseudo4",
            ArpMode::BassLock => "BassLock",
            ArpMode::MelodyLock => "MelodyLock",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "off" | "direct2" | "direct" => ArpMode::Off,
            "pseudo3" => ArpMode::Pseudo3,
            "pseudo4" => ArpMode::Pseudo4,
            "basslock" | "bass-lock" | "bass_lock" => ArpMode::BassLock,
            "melodylock" | "melody-lock" | "melody_lock" => ArpMode::MelodyLock,
            _ => ArpMode::BassLock,
        }
    }
}

/// 物理チャンネルへの割当結果
#[derive(Debug, Clone)]
pub struct Assignment {
    /// CHA に割り当てる VCH のインデックス (0-3)、None = 無音
    pub cha_idx: Option<usize>,
    /// CHB に割り当てる VCH のインデックス (0-3)、None = 無音
    pub chb_idx: Option<usize>,
}

/// アルペジエータ
pub struct Arpeggiator {
    pub mode: ArpMode,
    pub rate_hz: f32,
    /// アルペジオティックカウンタ (増加するのみ)
    tick: u32,
}

impl Arpeggiator {
    pub fn new(mode: ArpMode, rate_hz: f32) -> Self {
        Self {
            mode,
            rate_hz: rate_hz.max(1.0),
            tick: 0,
        }
    }

    /// 現在のティックに基づいて VCH→PCH を割り当てる
    pub fn assign(&self, vchs: &[VirtualChannel; 4]) -> Assignment {
        match self.mode {
            ArpMode::Off => self.assign_off(vchs),
            ArpMode::Pseudo3 => self.assign_pseudo3(vchs),
            ArpMode::Pseudo4 => self.assign_pseudo4(vchs),
            ArpMode::BassLock => self.assign_basslock(vchs),
            ArpMode::MelodyLock => self.assign_melodylock(vchs),
        }
    }

    /// ティックを 1 進める
    pub fn advance(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    pub fn tick(&self) -> u32 {
        self.tick
    }

    // ─── 各モードの割当ロジック ─────────────────────────────

    /// Off: VCH1→CHA, VCH2→CHB (固定)
    fn assign_off(&self, _vchs: &[VirtualChannel; 4]) -> Assignment {
        Assignment {
            cha_idx: Some(0), // VCH1
            chb_idx: Some(1), // VCH2
        }
    }

    /// Pseudo3: CHA が VCH1/VCH3 を交互、CHB=VCH2
    fn assign_pseudo3(&self, vchs: &[VirtualChannel; 4]) -> Assignment {
        let cha = if self.tick % 2 == 0 {
            active_or_fallback(vchs, &[0, 2], None)
        } else {
            active_or_fallback(vchs, &[2, 0], None)
        };
        let chb = active_or_fallback(vchs, &[1], cha);
        Assignment { cha_idx: cha, chb_idx: chb }
    }

    /// Pseudo4: 4 ティック周期で VCH1〜4 を時分割
    ///
    /// Tick 0: CHA=VCH1, CHB=VCH2
    /// Tick 1: CHA=VCH3, CHB=VCH4
    /// Tick 2: CHA=VCH1, CHB=VCH3
    /// Tick 3: CHA=VCH2, CHB=VCH4
    fn assign_pseudo4(&self, vchs: &[VirtualChannel; 4]) -> Assignment {
        let (cha_pref, chb_pref): (&[usize], &[usize]) = match self.tick % 4 {
            0 => (&[0], &[1]),
            1 => (&[2], &[3]),
            2 => (&[0], &[2]),
            3 => (&[1], &[3]),
            _ => unreachable!(),
        };
        let cha = active_or_fallback(vchs, cha_pref, None);
        let chb = active_or_fallback(vchs, chb_pref, cha);
        Assignment { cha_idx: cha, chb_idx: chb }
    }

    /// BassLock: CHB=VCH2 固定、CHA で VCH1/VCH3/VCH4 を時分割
    ///
    /// 要件定義 6.2 のパターン:
    ///   Tick 0: CHA=VCH1, CHB=VCH2
    ///   Tick 1: CHA=VCH3, CHB=VCH2
    ///   Tick 2: CHA=VCH1, CHB=VCH4
    ///   Tick 3: CHA=VCH3, CHB=VCH2
    fn assign_basslock(&self, vchs: &[VirtualChannel; 4]) -> Assignment {
        let (cha_pref, chb_pref): (&[usize], &[usize]) = match self.tick % 4 {
            0 => (&[0, 2, 3], &[1]),
            1 => (&[2, 0, 3], &[1]),
            2 => (&[0, 2], &[3, 1]),
            3 => (&[2, 0, 3], &[1]),
            _ => unreachable!(),
        };
        let cha = active_or_fallback(vchs, cha_pref, None);
        let chb = active_or_fallback(vchs, chb_pref, cha);
        Assignment { cha_idx: cha, chb_idx: chb }
    }

    /// MelodyLock: CHA=VCH1 固定、CHB で VCH2/VCH3/VCH4 を時分割
    fn assign_melodylock(&self, vchs: &[VirtualChannel; 4]) -> Assignment {
        let chb_pref: &[usize] = match self.tick % 4 {
            0 => &[1, 2, 3],
            1 => &[2, 1, 3],
            2 => &[1, 3, 2],
            3 => &[3, 1, 2],
            _ => unreachable!(),
        };
        let cha = active_or_fallback(vchs, &[0], None);
        let chb = active_or_fallback(vchs, chb_pref, cha);
        Assignment { cha_idx: cha, chb_idx: chb }
    }
}

/// 優先順位リストから最初のアクティブな VCH を返す。
/// `exclude` は既に割り当て済みの VCH インデックス。
fn active_or_fallback(
    vchs: &[VirtualChannel; 4],
    preferred: &[usize],
    exclude: Option<usize>,
) -> Option<usize> {
    // まずアクティブな優先チャンネルを探す
    for &idx in preferred {
        if exclude == Some(idx) {
            continue;
        }
        if vchs[idx].is_active() {
            return Some(idx);
        }
    }
    // アクティブでなければ enabled のものをフォールバック
    for &idx in preferred {
        if exclude == Some(idx) {
            continue;
        }
        if vchs[idx].enabled {
            return Some(idx);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::virtual_channel::VirtualChannel;

    fn make_vchs(active: &[bool]) -> [VirtualChannel; 4] {
        let mut vchs = [
            VirtualChannel::new(1),
            VirtualChannel::new(2),
            VirtualChannel::new(3),
            VirtualChannel::new(4),
        ];
        for (i, &a) in active.iter().enumerate() {
            if a {
                vchs[i].note_on(440.0, 1.0, f32::MAX, 0.0);
            }
        }
        vchs
    }

    #[test]
    fn test_basslock_tick0() {
        let arp = Arpeggiator::new(ArpMode::BassLock, 240.0);
        let vchs = make_vchs(&[true, true, true, false]);
        let a = arp.assign(&vchs);
        assert_eq!(a.cha_idx, Some(0)); // VCH1
        assert_eq!(a.chb_idx, Some(1)); // VCH2
    }

    #[test]
    fn test_off_mode() {
        let arp = Arpeggiator::new(ArpMode::Off, 240.0);
        let vchs = make_vchs(&[true, true, false, false]);
        let a = arp.assign(&vchs);
        assert_eq!(a.cha_idx, Some(0));
        assert_eq!(a.chb_idx, Some(1));
    }
}
