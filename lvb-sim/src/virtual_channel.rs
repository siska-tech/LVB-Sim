/// 論理チャンネル (Virtual Channel, VCH)
///
/// 物理チャンネルは 2ch だが、アルペジエータが高速に切り替えることで
/// 論理的には 4ch として動作する。

/// 楽器タイプ
#[derive(Debug, Clone, PartialEq)]
pub enum Instrument {
    /// 通常の矩形波音源
    Square,
    /// 短音パーカッション / クリック (CH-D / VCH4)
    Percussion,
}

/// パーカッション種別
///
/// μMML CH4 の `buffer1` (note nibble = サンプルID) から直接変換する。
/// 実機の 1bit サンプルを VIA 向けドラムマクロへ再合成する。
///
/// sample_index[] = {0, 19, 34, 74, 118, 126}
///   sample_id 1 → bwoop (bytes  0-18)
///   sample_id 2 → beep  (bytes 19-33)
///   sample_id 3 → kick  (bytes 34-73)
///   sample_id 4 → snare (bytes 74-117)
///   sample_id 5 → hi-hat(bytes 118-125)
#[derive(Debug, Clone, PartialEq)]
pub enum DrumType {
    /// Bwoop: 下降トーン (~900 Hz → ~200 Hz, τ=40ms)
    Bwoop,
    /// Beep: 固定トーン (~800 Hz, τ=15ms)
    Beep,
    /// キック: ピッチ急降下 (~1400 Hz → ~80 Hz, τ=20ms)
    Kick,
    /// スネア: 擬似ランダム周波数列 (1800-5000 Hz, τ=12ms)
    Snare,
    /// ハイハット: 高域擬似ランダム (5000-9000 Hz, τ=5ms)
    HiHat,
    /// クリック: 固定短パルス (2000 Hz, τ=3ms)
    Click,
}

impl DrumType {
    /// μMML CH4 の sample_id (buffer1) からドラム種別を決定する。
    ///
    /// μMML テキスト上の note nibble と sample_index[] の対応:
    ///   c=1(bwoop), c+=2(beep), d=3(kick), d+=4(snare), e=5(hi-hat)
    pub fn from_mmml_sample_id(id: u8) -> Self {
        match id {
            1 => DrumType::Bwoop,
            2 => DrumType::Beep,
            3 => DrumType::Kick,
            4 => DrumType::Snare,
            5 => DrumType::HiHat,
            _ => DrumType::Click,
        }
    }

    /// YAML 入力など sample_id が不明な場合の周波数ベースフォールバック。
    pub fn from_freq_fallback(hz: f32) -> Self {
        if hz < 500.0 {
            DrumType::Kick
        } else if hz < 1400.0 {
            DrumType::Snare
        } else if hz < 4000.0 {
            DrumType::HiHat
        } else {
            DrumType::Click
        }
    }
}

/// 論理チャンネルの状態
#[derive(Debug, Clone)]
pub struct VirtualChannel {
    /// チャンネル番号 (1-4)
    pub id: u8,
    /// 有効フラグ
    pub enabled: bool,
    /// 発振周波数 [Hz]
    pub frequency_hz: f32,
    /// 音量 [0.0, 1.0]
    pub volume: f32,
    /// ゲート: true = 発音中
    pub gate_open: bool,
    /// ゲートが閉じる時刻 [秒]
    pub gate_close_time: f32,
    /// 優先度 (値が大きいほど高優先)
    pub priority: u32,
    /// 楽器タイプ
    pub instrument: Instrument,
    /// 発音開始時刻 [秒] (パーカッション包絡線トリガー検出用)
    pub trigger_time: f32,
    /// ドラム種別 (Percussion チャンネルのみ有効)
    pub drum_type: DrumType,
}

impl VirtualChannel {
    pub fn new(id: u8) -> Self {
        let instrument = if id == 4 {
            Instrument::Percussion
        } else {
            Instrument::Square
        };
        Self {
            id,
            enabled: false,
            frequency_hz: 440.0,
            volume: 1.0,
            gate_open: false,
            gate_close_time: f32::MAX,
            priority: (4 - id as u32) * 3 + 1, // VCH1=10, VCH2=7, VCH3=4, VCH4=1
            instrument,
            trigger_time: f32::NEG_INFINITY,
            drum_type: DrumType::Click,
        }
    }

    /// このチャンネルが現在発音中かどうか
    #[inline]
    pub fn is_active(&self) -> bool {
        self.enabled && self.gate_open
    }

    /// ノートオン: 周波数・音量を設定してゲートを開く
    ///
    /// Percussion チャンネルは frequency_hz からドラム種別を自動分類する。
    pub fn note_on(&mut self, frequency_hz: f32, volume: f32, gate_close_time: f32, trigger_time: f32) {
        self.enabled = true;
        self.frequency_hz = frequency_hz;
        self.volume = volume.clamp(0.0, 1.0);
        self.gate_open = true;
        self.gate_close_time = gate_close_time;
        self.trigger_time = trigger_time;
        if self.instrument == Instrument::Percussion {
            self.drum_type = DrumType::from_freq_fallback(frequency_hz);
        }
    }

    /// ゲートを閉じる（サステインフェーズ終了）
    pub fn close_gate(&mut self) {
        self.gate_open = false;
    }

    /// チャンネルをリセット
    pub fn reset(&mut self) {
        self.enabled = false;
        self.gate_open = false;
        self.gate_close_time = f32::MAX;
    }

    /// 現在時刻に基づいてゲートを更新する
    pub fn update_gate(&mut self, current_time: f32) {
        if self.gate_open && current_time >= self.gate_close_time {
            self.gate_open = false;
        }
    }
}

/// 4つの論理チャンネルを管理するコンテナ
pub struct VirtualChannels {
    pub channels: [VirtualChannel; 4],
}

impl VirtualChannels {
    pub fn new() -> Self {
        Self {
            channels: [
                VirtualChannel::new(1),
                VirtualChannel::new(2),
                VirtualChannel::new(3),
                VirtualChannel::new(4),
            ],
        }
    }

    /// 全チャンネルのゲートを時刻に基づいて更新
    pub fn update_gates(&mut self, current_time: f32) {
        for ch in &mut self.channels {
            ch.update_gate(current_time);
        }
    }

    /// 指定チャンネル (1-4) への参照
    pub fn get(&self, id: u8) -> &VirtualChannel {
        &self.channels[(id.clamp(1, 4) - 1) as usize]
    }

    /// 指定チャンネル (1-4) への可変参照
    pub fn get_mut(&mut self, id: u8) -> &mut VirtualChannel {
        &mut self.channels[(id.clamp(1, 4) - 1) as usize]
    }
}

impl Default for VirtualChannels {
    fn default() -> Self {
        Self::new()
    }
}
