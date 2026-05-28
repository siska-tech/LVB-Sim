/// μMML CH4 パーカッション: 独立 1bit サンプル再生器
///
/// 元エンジン (mmml-engine.c) の方式を踏襲:
///   - SAMPLE_SPEED = 5: 1 bit を 5 オーディオサンプルで出力
///   - LSB ファースト: 各バイトの bit0 が最初に再生される
///   - bit=1 → +volume, bit=0 → 0.0 (DC オフセット方式)
///
/// アルペジエータから完全独立し、毎オーディオサンプルで bit を進める。
/// これにより 33ms のキックドラムが正しい長さで再生される。
///
/// サンプルデータはすべて独自設計。GPL ソースからの転載なし。

use crate::virtual_channel::DrumType;

/// 1 bit あたりのオーディオサンプル数 (元エンジンと同値)
const SAMPLE_SPEED: u32 = 5;

// ─────────────────────────────────────────────────────────
// 独自 1bit サンプルデータ
//
// 有効サンプルレート = audio_rate / SAMPLE_SPEED
//   48000 Hz / 5 = 9600 bit/s
//
// 周波数の目安:
//   交互 (10..):  9600/2 = 4800 Hz
//   4bit 周期 (1100..): 9600/4 = 2400 Hz
//   8bit 周期 (11110000..): 9600/8 = 1200 Hz
//   16bit (FF 00..): 9600/16 = 600 Hz
//   32bit (FF FF 00 00..): 9600/32 = 300 Hz
//   64bit (FF×4 00×4..): 9600/64 = 150 Hz
// ─────────────────────────────────────────────────────────

/// bwoop (sample_id=1) — ~21ms, 下降トーン 1200→150 Hz
///
/// 1200 Hz (3 cycles) → 600 Hz (2 cycles) → 300 Hz (2 cycles) → 150 Hz (1 cycle) → 無音
const BWOOP: &[u8] = &[
    // 1200 Hz: period=8bit, duty=50% → 0x0F = 0000_1111 LSB-first: 1,1,1,1,0,0,0,0
    0x0F, 0x0F, 0x0F,
    // 600 Hz: period=16bit → 0xFF, 0x00
    0xFF, 0x00, 0xFF, 0x00,
    // 300 Hz: period=32bit → 0xFF×2, 0x00×2
    0xFF, 0xFF, 0x00, 0x00,
    0xFF, 0xFF, 0x00, 0x00,
    // 150 Hz: period=64bit → 0xFF×4, 0x00×4
    0xFF, 0xFF, 0xFF, 0xFF,
    0x00, 0x00, 0x00, 0x00,
    // 無音テール
    0x00,
];

/// beep (sample_id=2) — ~15ms, 800 Hz 矩形波
///
/// 周期=12bit (6×1 + 6×0):
///   bit列: 1,1,1,1,1,1, 0,0,0,0,0,0, ...
///   バイト境界でずれるが 3 バイト (24bit = 2 cycle) で一致する:
///     0x3F=0b00111111 → 1,1,1,1,1,1,0,0
///     0xF0=0b11110000 → 0,0,0,0,1,1,1,1
///     0x03=0b00000011 → 1,1,0,0,0,0,0,0
///   この 3 バイトを 6 回繰り返すと 18 bytes = 144 bits ≈ 15ms
const BEEP: &[u8] = &[
    0x3F, 0xF0, 0x03,
    0x3F, 0xF0, 0x03,
    0x3F, 0xF0, 0x03,
    0x3F, 0xF0, 0x03,
    0x3F, 0xF0, 0x03,
    0x3F, 0xF0, 0x03,
];

/// kick (sample_id=3) — ~33ms, ピッチ急降下
///
/// 4800 Hz クリック → 2400 → 1200 → 600 → 300 → 150 Hz → 無音
const KICK: &[u8] = &[
    // 4800 Hz アタッククリック (3 bytes = 2.5ms)
    // 0x55 = 0101_0101 LSB-first: 1,0,1,0,1,0,1,0
    0x55, 0x55, 0x55,
    // 2400 Hz (2 bytes = 1.7ms)
    // 0x33 = 0011_0011 → 1,1,0,0,1,1,0,0
    0x33, 0x33,
    // 1200 Hz (4 bytes = 3.3ms, 4 cycles)
    0x0F, 0x0F, 0x0F, 0x0F,
    // 600 Hz (4 bytes = 3.3ms, 2 cycles)
    0xFF, 0x00, 0xFF, 0x00,
    // 300 Hz (8 bytes = 6.7ms, 2 cycles)
    0xFF, 0xFF, 0x00, 0x00,
    0xFF, 0xFF, 0x00, 0x00,
    // 150 Hz ボディ (16 bytes = 13.3ms, 2 cycles)
    0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00,
    0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00,
    // 無音テール (3 bytes)
    0x00, 0x00, 0x00,
];

/// snare (sample_id=4) — ~30ms, 擬似ランダムノイズ
///
/// アタック → ノイズボディ → フェードアウト。
/// bit 密度 ≈ 50%、規則性なし。元データの転載なし。
const SNARE: &[u8] = &[
    // アタック: 密度高め
    0xCA, 0x56, 0xD2, 0xAB,
    // ボディ: 擬似ランダムノイズ
    0x6D, 0xB4, 0x59, 0x36,
    0x8D, 0xC2, 0xB5, 0x6A,
    0xD3, 0x94, 0x5E, 0xA7,
    0x3C, 0xD6, 0xB1, 0x4A,
    0x9B, 0x25, 0xE8, 0x43,
    0xC7, 0x52, 0x8F, 0x1D,
    // フェード: 密度が落ちる
    0x64, 0x28, 0x10, 0x44,
    0x20, 0x00, 0x00, 0x00,
];

/// hi-hat (sample_id=5) — ~7ms, 高域ノイズバースト
///
/// 4800 Hz 交互パターン + ランダム → 無音で終端
const HIHAT: &[u8] = &[
    // 0xAA = 1010_1010 → 0,1,0,1,0,1,0,1 (4800 Hz)
    0xAA,
    // 0x55 = 0101_0101 → 1,0,1,0,1,0,1,0 (4800 Hz, 逆位相)
    0x55,
    // ランダムノイズ
    0x96, 0xD2, 0x4B, 0xA5,
    // フェード
    0x3C, 0x00,
];

/// click (fallback) — ~2.5ms, 短パルス
const CLICK: &[u8] = &[
    0x55, 0x00, 0x00,
];

fn drum_sample_data(drum_type: &DrumType) -> &'static [u8] {
    match drum_type {
        DrumType::Bwoop => BWOOP,
        DrumType::Beep  => BEEP,
        DrumType::Kick  => KICK,
        DrumType::Snare => SNARE,
        DrumType::HiHat => HIHAT,
        DrumType::Click => CLICK,
    }
}

// ─────────────────────────────────────────────────────────
// 再生器
// ─────────────────────────────────────────────────────────

/// 1bit サンプル再生器
///
/// アルペジエータから独立して動作する。
/// `generate_sample()` を毎オーディオサンプルで呼ぶことで、
/// arp のティミングに関わらず正しい長さでサンプルを再生する。
pub struct PercussionPlayer {
    active: bool,
    sample_data: &'static [u8],
    /// 現在の bit インデックス (LSB-first, 0 から)
    bit_pos: usize,
    /// SAMPLE_SPEED カウントダウン
    speed_count: u32,
    /// 最後に読んだビット値
    current_bit: bool,
    volume: f32,
}

impl PercussionPlayer {
    pub fn new() -> Self {
        Self {
            active: false,
            sample_data: CLICK,
            bit_pos: usize::MAX, // total_bits を超えているので即終了扱い
            speed_count: 0,
            current_bit: false,
            volume: 1.0,
        }
    }

    /// 新しいドラムヒットをトリガーする
    pub fn trigger(&mut self, volume: f32, drum_type: &DrumType) {
        self.sample_data = drum_sample_data(drum_type);
        self.bit_pos = 0;
        self.speed_count = 0;
        self.current_bit = false;
        self.volume = volume.clamp(0.0, 1.0);
        self.active = true;
    }

    /// 1 オーディオサンプルを生成する
    ///
    /// bit=1 → +volume, bit=0 → 0.0
    /// サンプル終端で自動的に非アクティブになる。
    pub fn generate_sample(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }

        let total_bits = self.sample_data.len() * 8;
        if self.bit_pos >= total_bits {
            self.active = false;
            return 0.0;
        }

        self.speed_count += 1;
        if self.speed_count >= SAMPLE_SPEED {
            self.speed_count = 0;
            let byte_idx = self.bit_pos / 8;
            let bit_idx = self.bit_pos % 8; // LSB ファースト
            self.current_bit = (self.sample_data[byte_idx] >> bit_idx) & 1 == 1;
            self.bit_pos += 1;
        }

        if self.current_bit { self.volume } else { 0.0 }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }
}

impl Default for PercussionPlayer {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────
// テスト
// ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// サンプルビット列を展開して長さを検証する
    fn sample_duration_ms(data: &[u8], audio_rate: u32) -> f32 {
        let total_bits = data.len() * 8;
        total_bits as f32 * SAMPLE_SPEED as f32 / audio_rate as f32 * 1000.0
    }

    #[test]
    fn test_kick_duration() {
        // キックは ~33ms でなければならない
        let dur = sample_duration_ms(KICK, 48000);
        assert!(dur > 30.0 && dur < 40.0, "kick duration: {:.1}ms", dur);
    }

    #[test]
    fn test_hihat_shorter_than_snare() {
        let hat_dur = sample_duration_ms(HIHAT, 48000);
        let snare_dur = sample_duration_ms(SNARE, 48000);
        assert!(hat_dur < snare_dur, "hi-hat({:.1}ms) < snare({:.1}ms)", hat_dur, snare_dur);
    }

    #[test]
    fn test_beep_800hz() {
        // BEEP は 800 Hz パターン: 3 バイト周期 = 24 bits = 2 cycles
        // 1 cycle = 12 bits → f = 9600/12 = 800 Hz
        // 最初の 24 bits が [0x3F, 0xF0, 0x03] と一致すること
        assert_eq!(&BEEP[..3], &[0x3F, 0xF0, 0x03]);
        assert_eq!(&BEEP[3..6], &[0x3F, 0xF0, 0x03]);
    }

    #[test]
    fn test_player_trigger_and_play() {
        let mut player = PercussionPlayer::new();
        assert!(!player.is_active());

        player.trigger(1.0, &DrumType::Click);
        assert!(player.is_active());

        // CLICK = [0x55, 0x00, 0x00] = 24 bits
        // SAMPLE_SPEED=5 → 最後の bit は sample 120 で読まれ、
        // 終端検出は sample 121 の先頭で起きる。
        let total_bits = CLICK.len() * 8;
        let end_sample = total_bits * SAMPLE_SPEED as usize + 1; // +1 for termination detection
        let mut saw_nonzero = false;
        for _ in 0..end_sample {
            let s = player.generate_sample();
            if s > 0.0 { saw_nonzero = true; }
        }
        assert!(saw_nonzero, "少なくとも 1 サンプルは非ゼロであること");
        assert!(!player.is_active(), "{} サンプル後に終了していること", end_sample);
    }

    #[test]
    fn test_player_kick_lasts_longer_than_hihat() {
        let mut kick = PercussionPlayer::new();
        let mut hat = PercussionPlayer::new();

        kick.trigger(1.0, &DrumType::Kick);
        hat.trigger(1.0, &DrumType::HiHat);

        // 終端検出は最終 bit 読み込みの次の呼び出しで起きる (+1)
        let hat_end = HIHAT.len() * 8 * SAMPLE_SPEED as usize + 1;
        for _ in 0..hat_end {
            kick.generate_sample();
            hat.generate_sample();
        }
        assert!(!hat.is_active(), "hi-hat は {} samples で終了", hat_end);
        assert!(kick.is_active(), "kick はまだ再生中");
    }

    #[test]
    fn test_lsb_first_kick_starts_with_click() {
        // キックの最初の 8 bit (byte 0 = 0x55) は交互パターン (4800 Hz クリック)
        // 0x55 = 01010101, LSB-first → 1,0,1,0,1,0,1,0
        let mut player = PercussionPlayer::new();
        player.trigger(1.0, &DrumType::Kick);

        let mut bits = Vec::new();
        for _ in 0..(8 * SAMPLE_SPEED as usize) {
            // SAMPLE_SPEED サンプルにつき 1 bit 変化する
            // 各ブロックの最後のサンプルが確定値
            let s = player.generate_sample();
            bits.push(s > 0.5);
        }

        // 最初の 8 bit を確認: 各 SAMPLE_SPEED サンプルブロックの最後
        let first_byte_bits: Vec<bool> = (0..8)
            .map(|b| bits[b * SAMPLE_SPEED as usize + (SAMPLE_SPEED as usize - 1)])
            .collect();
        // 0x55 LSB-first: 1,0,1,0,1,0,1,0
        let expected = [true, false, true, false, true, false, true, false];
        assert_eq!(&first_byte_bits[..], &expected, "キック先頭 8 bit が 0x55 LSB-first と一致すること");
    }
}
