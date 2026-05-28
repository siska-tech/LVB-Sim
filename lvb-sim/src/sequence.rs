/// シーケンスデータ — 音符イベントの時系列リスト
///
/// YAML 入力と mmml パーサの共通出力形式。

use anyhow::{anyhow, Result};
use serde::Deserialize;

use crate::virtual_channel::Instrument;

// ─────────────────────────────────────────────────────────
// コアデータ型
// ─────────────────────────────────────────────────────────

/// 1 つの音符イベント
#[derive(Debug, Clone)]
pub struct SongEvent {
    /// 発音開始時刻 [秒]
    pub time_secs: f32,
    /// ゲートが閉じる時刻 [秒] (発音終了)
    pub gate_close_secs: f32,
    /// 対象論理チャンネル (1-4)
    pub vchannel: u8,
    /// 発振周波数 [Hz]
    pub frequency_hz: f32,
    /// 音量 [0.0, 1.0]
    pub volume: f32,
    /// 楽器タイプ
    pub instrument: Instrument,
}

/// 曲全体のシーケンス
#[derive(Debug)]
pub struct Sequence {
    pub title: Option<String>,
    pub author: Option<String>,
    /// 時系列でソート済みのイベント一覧
    pub events: Vec<SongEvent>,
    /// 全体の長さ [秒]
    pub total_duration_secs: f32,
    /// 元データのテンポ [BPM]
    pub source_tempo_bpm: f32,
}

impl Sequence {
    /// イベントを時刻順にソートして重複を解消
    pub fn finalize(&mut self) {
        self.events.sort_by(|a, b| {
            a.time_secs
                .partial_cmp(&b.time_secs)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// 曲の実際の長さを gate_close_secs から計算
    pub fn compute_duration(&mut self, tail_secs: f32) {
        self.total_duration_secs = self
            .events
            .iter()
            .map(|e| e.gate_close_secs)
            .fold(0.0f32, f32::max)
            + tail_secs;
    }
}

// ─────────────────────────────────────────────────────────
// YAML 入力形式
// ─────────────────────────────────────────────────────────

/// YAML ファイルのルートノード
#[derive(Debug, Deserialize)]
pub struct YamlSong {
    pub title: Option<String>,
    pub author: Option<String>,
    /// テンポ [BPM]
    pub tempo: f32,
    #[serde(default)]
    pub tracks: Vec<YamlTrack>,
}

/// トラック (チャンネル単位)
#[derive(Debug, Deserialize)]
pub struct YamlTrack {
    /// 論理チャンネル番号 (1-4)
    pub channel: u8,
    /// 優先度 (省略時 = チャンネル番号に応じたデフォルト)
    pub priority: Option<u32>,
    /// 楽器: "square" / "percussion" (省略時 = "square")
    pub instrument: Option<String>,
    pub events: Vec<YamlEvent>,
}

/// 1 つの音符または休符
#[derive(Debug, Deserialize)]
pub struct YamlEvent {
    /// 音符名 "C4", "A#3", "Bb4" など (note か freq のどちらか必須)
    pub note: Option<String>,
    /// 直接 Hz で指定する場合
    pub freq: Option<f32>,
    /// 休符の場合 true
    pub rest: Option<bool>,
    /// 音符の長さ: 1=全音符, 2=二分音符, 4=四分音符, 8=八分音符 …
    pub length: Option<u32>,
    /// 付点 (true = 長さ×1.5)
    pub dotted: Option<bool>,
    /// 音量 0-15 (省略時 = 12)
    pub volume: Option<u32>,
    /// ゲート長 0.0-1.0 (省略時はデフォルト値)
    pub gate: Option<f32>,
}

// ─────────────────────────────────────────────────────────
// 音符名 → 周波数 変換
// ─────────────────────────────────────────────────────────

/// "C4", "A#3", "Bb4", "D+2" などの音符名を Hz に変換
///
/// MIDI 規則: C4 = MIDI 60 = 261.63 Hz、A4 = MIDI 69 = 440 Hz
pub fn parse_note_name(name: &str) -> Result<f32> {
    let s = name.trim();
    let mut chars = s.chars();

    let note_char = chars
        .next()
        .ok_or_else(|| anyhow!("音符名が空です"))?
        .to_ascii_uppercase();

    let semitone: i32 = match note_char {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return Err(anyhow!("無効な音符文字: {}", note_char)),
    };

    let rest = chars.as_str();

    let (accidental, octave_str) = if rest.starts_with('#') || rest.starts_with('+') {
        (1i32, &rest[1..])
    } else if rest.starts_with('b') && rest.len() > 1 && rest.as_bytes().get(1).map_or(false, |c| c.is_ascii_digit()) {
        (-1i32, &rest[1..])
    } else {
        (0i32, rest)
    };

    let octave: i32 = octave_str
        .parse()
        .map_err(|_| anyhow!("無効なオクターブ: '{}'", octave_str))?;

    // MIDI ノート番号: C4=60, A4=69
    let midi = 12 * (octave + 1) + semitone + accidental;
    let freq = 440.0 * 2.0f32.powf((midi - 69) as f32 / 12.0);
    Ok(freq)
}

// ─────────────────────────────────────────────────────────
// YAML → Sequence 変換
// ─────────────────────────────────────────────────────────

impl Sequence {
    pub fn from_yaml(yaml: YamlSong) -> Result<Self> {
        let tempo = yaml.tempo;
        if tempo <= 0.0 {
            return Err(anyhow!("テンポは正の値にしてください: {}", tempo));
        }
        let beat_secs = 60.0 / tempo; // 1 拍あたりの秒数

        let mut events = Vec::new();

        for track in &yaml.tracks {
            let vchannel = track.channel.clamp(1, 4);
            let instrument = match track.instrument.as_deref() {
                Some("percussion") => Instrument::Percussion,
                _ => Instrument::Square,
            };
            let default_gate = match instrument {
                Instrument::Percussion => 0.05,  // 短音: 50ms
                Instrument::Square => 0.875,     // 通常: 7/8 ゲート
            };

            let mut time_beats = 0.0f32;

            for ev in &track.events {
                let length = ev.length.unwrap_or(4).clamp(1, 128);
                let dotted = ev.dotted.unwrap_or(false);
                let duration_beats = 4.0 / length as f32 * if dotted { 1.5 } else { 1.0 };
                let duration_secs = duration_beats * beat_secs;

                let is_rest = ev.rest.unwrap_or(false);

                if !is_rest {
                    let freq = if let Some(f) = ev.freq {
                        f
                    } else if let Some(ref name) = ev.note {
                        parse_note_name(name)?
                    } else {
                        return Err(anyhow!("note も freq もありません"));
                    };

                    let volume = ev.volume.unwrap_or(12).clamp(0, 15) as f32 / 15.0;
                    let gate_length = ev.gate.unwrap_or(default_gate).clamp(0.0, 1.0);
                    let time_secs = time_beats * beat_secs;
                    let gate_close_secs = time_secs + duration_secs * gate_length;

                    events.push(SongEvent {
                        time_secs,
                        gate_close_secs,
                        vchannel,
                        frequency_hz: freq,
                        volume,
                        instrument: instrument.clone(),
                    });
                }

                time_beats += duration_beats;
            }
        }

        events.sort_by(|a, b| {
            a.time_secs
                .partial_cmp(&b.time_secs)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let total_duration_secs = events
            .iter()
            .map(|e| e.gate_close_secs)
            .fold(0.0f32, f32::max)
            + 1.0; // 末尾に 1 秒余白

        Ok(Sequence {
            title: yaml.title,
            author: yaml.author,
            events,
            total_duration_secs,
            source_tempo_bpm: tempo,
        })
    }
}

// ─────────────────────────────────────────────────────────
// テスト
// ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a4_440hz() {
        let f = parse_note_name("A4").unwrap();
        assert!((f - 440.0).abs() < 0.1, "A4 = {:.2}", f);
    }

    #[test]
    fn test_c4_middle_c() {
        let f = parse_note_name("C4").unwrap();
        assert!((f - 261.63).abs() < 0.5, "C4 = {:.2}", f);
    }

    #[test]
    fn test_sharp() {
        let f1 = parse_note_name("A#4").unwrap();
        let f2 = parse_note_name("Bb4").unwrap();
        // A#4 と Bb4 は同じ音
        assert!((f1 - f2).abs() < 0.1, "A#4={:.2} Bb4={:.2}", f1, f2);
    }
}
