/// μMML / mmml フォーマットパーサ
///
/// 処理フロー:
///   1. `%` コメントをストリップ
///   2. `@` でチャンネル / マクロに分割
///   3. ループ [n...] とマクロ m1〜m255 を展開
///   4. 展開済みテキストを音符イベントに変換
///   5. Sequence を構築

use anyhow::{anyhow, Result};

use crate::sequence::{Sequence, SongEvent};
use crate::virtual_channel::Instrument;

// ─────────────────────────────────────────────────────────
// mmml 周波数テーブル
//
// 実機: f = (215000 × (1 << octave)) / note_table[note_idx]
// octave は mmml の o1〜o5 = 1〜5
// note_table: C C# D D# E F F# G G# A A# B
// ─────────────────────────────────────────────────────────
const NOTE_TABLE: [u32; 12] = [
    1644, 1551, 1464, 1382, 1305, 1231, 1162, 1097, 1035, 977, 922, 871,
];
const MMML_RATE: f32 = 215_000.0;

/// mmml 音符インデックス (0-11: C〜B) と オクターブ (1-5) から Hz を計算
fn mmml_freq(note_idx: usize, octave: u8) -> f32 {
    let oct = octave.clamp(1, 5) as u32;
    let octave_factor = 1u32 << oct;
    MMML_RATE * octave_factor as f32 / NOTE_TABLE[note_idx % 12] as f32
}

/// mmml テンポ値 (t コマンドの引数) → BPM 変換
///
/// デスクトップシンセサイザーの実測から逆算:
///   BPM = 100781.25 / (t_value × 16 + 1)
///
/// 例: t36 → ~175 BPM ("Fly Me to the Moon")
///     t52 → ~121 BPM ("Sunglasses Snake")
fn tempo_to_bpm(t_value: u32) -> f32 {
    if t_value == 0 {
        return 120.0;
    }
    100_781.25 / (t_value as f32 * 16.0 + 1.0)
}

// ─────────────────────────────────────────────────────────
// ステップ 1: コメントストリップ
// ─────────────────────────────────────────────────────────

fn strip_comments(text: &str) -> String {
    text.lines()
        .map(|line| {
            if let Some(pos) = line.find('%') {
                &line[..pos]
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ─────────────────────────────────────────────────────────
// ステップ 2: @ でセクション分割
// ─────────────────────────────────────────────────────────

fn split_sections(text: &str) -> Vec<String> {
    // '@' を区切り文字としてテキストを分割し、各セクションを返す
    // (先頭の空白部分は捨てる)
    text.split('@')
        .skip(1) // '@' の前の部分は不要
        .map(|s| s.to_string())
        .collect()
}

// ─────────────────────────────────────────────────────────
// ステップ 3: ループとマクロの展開
// ─────────────────────────────────────────────────────────

/// テキスト中の数値を読む (先頭インデックスから)
fn read_u32(chars: &[char], start: usize) -> Option<(u32, usize)> {
    let mut i = start;
    let mut s = String::new();
    while i < chars.len() && chars[i].is_ascii_digit() {
        s.push(chars[i]);
        i += 1;
    }
    if s.is_empty() {
        None
    } else {
        s.parse::<u32>().ok().map(|n| (n, i))
    }
}

/// ループとマクロ呼び出しを展開する (再帰対応)
///
/// depth 制限により無限ループを防止する。
fn expand(text: &str, macros: &[String], depth: u32) -> String {
    const MAX_DEPTH: u32 = 16;
    const MAX_LOOP_COUNT: u32 = 255;

    if depth > MAX_DEPTH {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len() * 2);
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            // マクロ呼び出し: m<number>
            'm' if i + 1 < chars.len() && chars[i + 1].is_ascii_digit() => {
                i += 1;
                if let Some((n, ni)) = read_u32(&chars, i) {
                    i = ni;
                    let midx = n as usize;
                    if midx >= 1 && midx <= macros.len() {
                        let macro_text = macros[midx - 1].clone();
                        let expanded = expand_loops_only(&macro_text, depth + 1);
                        // \x01/\x02 でテンポのスコープを囲む (マクロ内テンポ変化を局所化)
                        result.push('\x01');
                        result.push_str(&expanded);
                        result.push('\x02');
                    }
                } else {
                    result.push('m');
                }
            }

            // ループ開始: [<count> ... ]
            '[' => {
                i += 1;
                // ループ回数を読む
                let (count, ni) = read_u32(&chars, i).unwrap_or((2, i));
                i = ni;

                // 対応する ']' を探す (ネストを考慮)
                let inner_start = i;
                let mut depth_bracket: u32 = 1;
                while i < chars.len() && depth_bracket > 0 {
                    if chars[i] == '[' {
                        depth_bracket += 1;
                    } else if chars[i] == ']' {
                        depth_bracket -= 1;
                    }
                    if depth_bracket > 0 {
                        i += 1;
                    }
                }
                let inner: String = chars[inner_start..i].iter().collect();
                if i < chars.len() {
                    i += 1; // ']' をスキップ
                }

                // 内部を展開してから count 回繰り返す
                let expanded_inner = expand(&inner, macros, depth + 1);
                let repeat = count.clamp(0, MAX_LOOP_COUNT) as usize;
                result.reserve(expanded_inner.len() * repeat);
                for _ in 0..repeat {
                    result.push_str(&expanded_inner);
                }
            }

            c => {
                result.push(c);
                i += 1;
            }
        }
    }

    result
}

/// ループのみ展開 (マクロ呼び出しはそのまま通す)
fn expand_loops_only(text: &str, depth: u32) -> String {
    const MAX_DEPTH: u32 = 16;
    const MAX_LOOP_COUNT: u32 = 255;

    if depth > MAX_DEPTH {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len() * 2);
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '[' {
            i += 1;
            let (count, ni) = read_u32(&chars, i).unwrap_or((2, i));
            i = ni;

            let inner_start = i;
            let mut depth_bracket: u32 = 1;
            while i < chars.len() && depth_bracket > 0 {
                if chars[i] == '[' {
                    depth_bracket += 1;
                } else if chars[i] == ']' {
                    depth_bracket -= 1;
                }
                if depth_bracket > 0 {
                    i += 1;
                }
            }
            let inner: String = chars[inner_start..i].iter().collect();
            if i < chars.len() {
                i += 1;
            }

            let expanded_inner = expand_loops_only(&inner, depth + 1);
            let repeat = count.clamp(0, MAX_LOOP_COUNT) as usize;
            for _ in 0..repeat {
                result.push_str(&expanded_inner);
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

// ─────────────────────────────────────────────────────────
// ステップ 4: 音符テキストの解析
// ─────────────────────────────────────────────────────────

/// 音符テキストの解析状態
struct ParseState {
    octave: u8,       // 1-5 (デフォルト: 3)
    volume: u8,       // 0-8 (デフォルト: 8 = 50% デューティ)
    last_length: u32, // 最後に明示された長さ (デフォルト: 4 = 四分音符)
    time_secs: f32,   // 現在の時刻 [秒] — テンポ変化に対して正確に積算
    tempo_bpm: f32,   // 現在のテンポ
}

impl ParseState {
    fn new(initial_tempo_bpm: f32) -> Self {
        Self {
            octave: 3,
            volume: 8,
            last_length: 4,
            time_secs: 0.0,
            tempo_bpm: initial_tempo_bpm,
        }
    }

    /// 拍数を現在のテンポで秒に変換
    fn beats_to_secs(&self, beats: f32) -> f32 {
        beats * 60.0 / self.tempo_bpm
    }

    /// 時刻を beats 分だけ進める (テンポ変化を正しく反映)
    fn advance(&mut self, beats: f32) {
        self.time_secs += self.beats_to_secs(beats);
    }
}

/// 長さの読み取り: 現在位置から数値と付点を解析
fn read_length(chars: &[char], i: usize, default: u32) -> (u32, bool, usize) {
    let (length, mut j) = read_u32(chars, i).unwrap_or((default, i));
    let length = length.clamp(1, 128);
    let dotted = j < chars.len() && chars[j] == '.';
    if dotted {
        j += 1;
    }
    (length, dotted, j)
}

/// 長さ値と付点から拍数を計算
fn length_to_beats(length: u32, dotted: bool) -> f32 {
    let base = 4.0 / length as f32;
    if dotted {
        base * 1.5
    } else {
        base
    }
}

/// チャンネル MML テキストを解析して SongEvent のリストを生成
fn parse_channel(
    text: &str,
    vchannel: u8,
    default_tempo_bpm: f32,
    instrument: Instrument,
) -> Vec<SongEvent> {
    let mut events: Vec<SongEvent> = Vec::new();
    let mut state = ParseState::new(default_tempo_bpm);
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    // マクロスコープ用テンポスタック (\x01 で push, \x02 で pop)
    let mut tempo_stack: Vec<f32> = Vec::new();

    // タイ処理用: 前のイベントのインデックス
    let mut tie_pending = false;
    let mut last_event_idx: Option<usize> = None;
    // タイ開始時刻 [秒]
    let mut tie_start_secs: f32 = 0.0;
    // タイ中の合計時間 [秒]
    let mut tie_total_secs: f32 = 0.0;

    // デフォルトゲート長
    let default_gate = match instrument {
        Instrument::Percussion => 0.05,
        Instrument::Square => 0.875,
    };

    while i < chars.len() {
        let c = chars[i];

        // ─── 空白・改行はスキップ ───────────────────────────
        if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
            i += 1;
            continue;
        }

        // ─── マクロスコープ: テンポを保存/復元 ─────────────
        if c == '\x01' {
            tempo_stack.push(state.tempo_bpm);
            i += 1;
            continue;
        }
        if c == '\x02' {
            if let Some(saved) = tempo_stack.pop() {
                state.tempo_bpm = saved;
            }
            i += 1;
            continue;
        }

        // ─── テンポ: t<number> ──────────────────────────────
        if c == 't' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
            i += 1;
            if let Some((n, ni)) = read_u32(&chars, i) {
                state.tempo_bpm = tempo_to_bpm(n);
                i = ni;
            }
            continue;
        }

        // ─── オクターブ: o<number> ──────────────────────────
        if c == 'o' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
            i += 1;
            if let Some((n, ni)) = read_u32(&chars, i) {
                state.octave = n.clamp(1, 5) as u8;
                i = ni;
            }
            continue;
        }

        // ─── オクターブ上下: > / < ──────────────────────────
        if c == '>' {
            if state.octave < 5 {
                state.octave += 1;
            }
            i += 1;
            continue;
        }
        if c == '<' {
            if state.octave > 1 {
                state.octave -= 1;
            }
            i += 1;
            continue;
        }

        // ─── 音量: v<number> ────────────────────────────────
        if c == 'v' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
            i += 1;
            if let Some((n, ni)) = read_u32(&chars, i) {
                state.volume = n.clamp(0, 8) as u8;
                i = ni;
            }
            continue;
        }

        // ─── タイ: & ────────────────────────────────────────
        if c == '&' {
            tie_pending = true;
            i += 1;
            continue;
        }

        // ─── 休符: r ────────────────────────────────────────
        if c == 'r' {
            i += 1;
            let (length, dotted, ni) = read_length(&chars, i, state.last_length);
            state.last_length = length;
            i = ni;
            let beats = length_to_beats(length, dotted);
            state.advance(beats);
            // タイはキャンセル
            tie_pending = false;
            last_event_idx = None;
            continue;
        }

        // ─── 音符: c, d, e, f, g, a, b ─────────────────────
        let note_semitone: Option<usize> = match c {
            'c' => Some(0),
            'd' => Some(2),
            'e' => Some(4),
            'f' => Some(5),
            'g' => Some(7),
            'a' => Some(9),
            'b' => Some(11),
            _ => None,
        };

        if let Some(mut semitone) = note_semitone {
            i += 1;
            // シャープ: c+ など
            if i < chars.len() && chars[i] == '+' {
                semitone += 1;
                i += 1;
            }
            semitone %= 12;

            let (length, dotted, ni) = read_length(&chars, i, state.last_length);
            state.last_length = length;
            i = ni;

            let duration_beats = length_to_beats(length, dotted);
            let duration_secs = state.beats_to_secs(duration_beats);
            let freq = mmml_freq(semitone, state.octave);
            let vol = if state.volume == 0 {
                0.0
            } else {
                state.volume as f32 / 8.0
            };

            if tie_pending {
                // タイ: 前のノートの gate_close を延長
                if let Some(idx) = last_event_idx {
                    tie_total_secs += duration_secs;
                    let gate_close_secs = tie_start_secs + tie_total_secs * default_gate;
                    events[idx].gate_close_secs = gate_close_secs.max(events[idx].gate_close_secs);
                }
                tie_pending = false;
            } else {
                // 通常のノートオン
                tie_start_secs = state.time_secs;
                tie_total_secs = duration_secs;
                let gate_close_secs = state.time_secs + duration_secs * default_gate;

                last_event_idx = Some(events.len());
                events.push(SongEvent {
                    time_secs: state.time_secs,
                    gate_close_secs,
                    vchannel,
                    frequency_hz: freq,
                    volume: vol,
                    instrument: instrument.clone(),
                });
            }

            state.advance(duration_beats);
            continue;
        }

        // ─── その他はスキップ ────────────────────────────────
        i += 1;
    }

    events
}

// ─────────────────────────────────────────────────────────
// ステップ 5: グローバルテンポの検出
// ─────────────────────────────────────────────────────────

/// 音符/休符より前に現れるテンポコマンドを探す (チャンネル初期テンポ)
///
/// mmml エンジンではテンポはグローバル。チャンネル D が `t46 r4` で始まる場合、
/// t46 は演奏開始直後に全チャンネルへ即時反映される。
/// この関数は各チャンネル先頭の音符・休符より前の `t<n>` を返す。
fn find_initial_tempo(sections: &[String]) -> Option<f32> {
    const NOTE_CHARS: &[char] = &['c', 'd', 'e', 'f', 'g', 'a', 'b', 'r'];
    for section in sections.iter().take(4) {
        let chars: Vec<char> = section.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
                i += 1;
                continue;
            }
            // 音符・休符が来たら、このチャンネルには初期テンポなし
            if NOTE_CHARS.contains(&c) {
                break;
            }
            if c == 't' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                i += 1;
                if let Some((n, _)) = read_u32(&chars, i) {
                    if n > 0 {
                        return Some(tempo_to_bpm(n));
                    }
                }
            }
            i += 1;
        }
    }
    None
}

/// テキスト中の最初の `t<number>` コマンドを探してテンポ [BPM] を返す (フォールバック)
fn find_first_tempo(sections: &[String]) -> Option<f32> {
    for section in sections {
        let chars: Vec<char> = section.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == 't' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                i += 1;
                if let Some((n, _)) = read_u32(&chars, i) {
                    if n > 0 {
                        return Some(tempo_to_bpm(n));
                    }
                }
            }
            i += 1;
        }
    }
    None
}

// ─────────────────────────────────────────────────────────
// メインエントリポイント
// ─────────────────────────────────────────────────────────

/// mmml ファイルテキストを解析して Sequence を返す
pub fn parse_mmml_file(content: &str) -> Result<Sequence> {
    // 1. コメントストリップ
    let stripped = strip_comments(content);

    // 2. @ でセクション分割
    let sections = split_sections(&stripped);

    if sections.is_empty() {
        return Err(anyhow!("mmml ファイルにチャンネルデータがありません (@が見つかりません)"));
    }

    // 3. チャンネル A-D (最初の 4 つ) とマクロを分離
    let num_channels = sections.len().min(4);
    let channel_sections: Vec<&str> = sections[..num_channels].iter().map(|s| s.as_str()).collect();
    let macro_sections: Vec<String> = if sections.len() > 4 {
        sections[4..].to_vec()
    } else {
        Vec::new()
    };

    // 4. グローバルテンポを検出
    // 音符より前に t<n> があればそれを初期テンポとして採用 (mmml エンジンはテンポがグローバル)
    // なければ全体スキャンの最初のテンポにフォールバック
    let global_tempo_bpm = find_initial_tempo(&sections)
        .or_else(|| find_first_tempo(&sections))
        .unwrap_or(120.0);

    // 5. 各チャンネルを展開・解析
    let mut all_events = Vec::new();

    for (ch_idx, &ch_text) in channel_sections.iter().enumerate() {
        let vchannel = (ch_idx as u8) + 1; // 1-4
        let instrument = if ch_idx == 3 {
            Instrument::Percussion
        } else {
            Instrument::Square
        };

        // マクロ + ループを展開
        let expanded = expand(ch_text, &macro_sections, 0);

        // 音符イベントに変換
        let events = parse_channel(&expanded, vchannel, global_tempo_bpm, instrument);
        all_events.extend(events);
    }

    // 6. 時刻順にソート
    all_events.sort_by(|a, b| {
        a.time_secs
            .partial_cmp(&b.time_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 7. 全体の長さを計算
    let total_duration_secs = all_events
        .iter()
        .map(|e| e.gate_close_secs)
        .fold(0.0f32, f32::max)
        + 1.0; // 末尾に 1 秒余白

    Ok(Sequence {
        title: None,
        author: None,
        events: all_events,
        total_duration_secs,
        source_tempo_bpm: global_tempo_bpm,
    })
}

// ─────────────────────────────────────────────────────────
// テスト
// ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tempo_conversion() {
        // t36 → ~175 BPM
        let bpm = tempo_to_bpm(36);
        assert!((bpm - 174.7).abs() < 1.0, "t36 → {:.1} BPM", bpm);

        // t52 → ~121 BPM
        let bpm = tempo_to_bpm(52);
        assert!((bpm - 121.0).abs() < 1.0, "t52 → {:.1} BPM", bpm);
    }

    #[test]
    fn test_a4_440hz() {
        // o1, note A (semitone 9) → 440 Hz
        let f = mmml_freq(9, 1);
        assert!((f - 440.0).abs() < 1.0, "o1 A = {:.2} Hz", f);
    }

    #[test]
    fn test_c_octave_doubling() {
        // 各オクターブで周波数が 2 倍になること
        let f1 = mmml_freq(0, 1); // o1 C
        let f2 = mmml_freq(0, 2); // o2 C
        let f3 = mmml_freq(0, 3); // o3 C
        assert!((f2 / f1 - 2.0).abs() < 0.01, "o1→o2 C: {:.2}/{:.2}", f2, f1);
        assert!((f3 / f2 - 2.0).abs() < 0.01, "o2→o3 C: {:.2}/{:.2}", f3, f2);
    }

    #[test]
    fn test_simple_parse() {
        let mmml = "@ t52 o1 c4 d4 e4 r4\n@\n@\n@";
        let seq = parse_mmml_file(mmml).unwrap();
        assert_eq!(seq.events.len(), 3, "3 音符が解析されること");
        assert!(seq.events[0].frequency_hz > 200.0 && seq.events[0].frequency_hz < 300.0);
    }

    #[test]
    fn test_loop_expansion() {
        // [3 c4 ] → 3 回繰り返し
        let mmml = "@ o1 [3 c4 ]\n@\n@\n@";
        let seq = parse_mmml_file(mmml).unwrap();
        assert_eq!(seq.events.len(), 3);
    }

    #[test]
    fn test_sharp_note() {
        // c+ (c#) の周波数は c より高い
        let f_c = mmml_freq(0, 1);
        let f_cs = mmml_freq(1, 1);
        assert!(f_cs > f_c, "c# > c");
    }

    #[test]
    fn test_octave_commands() {
        // > でオクターブが上がること
        let mmml = "@ o2 c4 > c4\n@\n@\n@";
        let seq = parse_mmml_file(mmml).unwrap();
        assert_eq!(seq.events.len(), 2);
        let f1 = seq.events[0].frequency_hz;
        let f2 = seq.events[1].frequency_hz;
        assert!((f2 / f1 - 2.0).abs() < 0.05, "> でオクターブ倍: {:.2}/{:.2}", f2, f1);
    }

    #[test]
    fn test_strip_comments() {
        let text = "c4 % this is a comment\nd4";
        let stripped = strip_comments(text);
        assert!(!stripped.contains('%'), "コメントが除去されること");
        assert!(stripped.contains('d'), "音符が残ること");
    }

    #[test]
    fn test_macro_expansion() {
        // m1 でマクロを展開
        let mmml = "@ o1 m1\n@\n@\n@\n@ c4 d4"; // macro #1 = c4 d4
        let seq = parse_mmml_file(mmml).unwrap();
        assert_eq!(seq.events.len(), 2, "マクロ展開後に 2 音符");
    }
}
