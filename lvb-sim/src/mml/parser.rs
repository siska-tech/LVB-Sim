/// μMML / mmml フォーマットパーサ
///
/// 処理フロー:
///   1. `%` コメントをストリップ
///   2. `@` でチャンネル / マクロに分割
///   3. ループ [n...] とマクロ m1〜m255 を展開
///   4. 全チャンネルからグローバルテンポタイムラインを構築
///   5. 展開済みテキストをグローバルテンポを参照して音符イベントに変換
///   6. Sequence を構築
///
/// テンポはグローバル変数として扱われる (実機 mmml エンジンと同様)。
/// いずれかのチャンネルで `t<n>` が現れると、全チャンネルのテンポが即時変わる。

use anyhow::{anyhow, Result};

use crate::sequence::{Sequence, SongEvent};
use crate::virtual_channel::{DrumType, Instrument};

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
                        result.push_str(&expanded);
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
// ステップ 4: グローバルテンポタイムラインの構築
// ─────────────────────────────────────────────────────────

/// 長さ値と付点から拍数を計算
fn length_to_beats(length: u32, dotted: bool) -> f32 {
    let base = 4.0 / length as f32;
    if dotted { base * 1.5 } else { base }
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

/// 1 チャンネルの展開済みテキストを事前スキャンし、
/// (絶対時刻 [秒], BPM) のリストを返す。
///
/// `context` が空でない場合、音符/休符の時間計算にグローバルタイムラインを参照する
/// （二パス目で使用）。これにより他チャンネルのテンポ変化が時刻計算に反映される。
///
/// t255 はチャンネルローカルな終端マーカーとして扱い、グローバルタイムラインには含めない。
fn extract_tempo_events(expanded: &str, initial_bpm: f32, context: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let chars: Vec<char> = expanded.chars().collect();
    let mut i = 0;
    let mut local_bpm = initial_bpm;
    let mut time_secs = 0.0f32;
    let mut last_length = 4u32;
    let mut result: Vec<(f32, f32)> = Vec::new();

    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\n' | '\r' => { i += 1; }

            // テンポコマンド: 時刻と BPM を記録
            // t255 はチャンネルローカルな終端マーカーのため伝播しない
            't' if i + 1 < chars.len() && chars[i + 1].is_ascii_digit() => {
                i += 1;
                if let Some((n, ni)) = read_u32(&chars, i) {
                    if n == 255 {
                        break;
                    }
                    local_bpm = tempo_to_bpm(n);
                    result.push((time_secs, local_bpm));
                    i = ni;
                }
            }

            // オクターブ・音量: 時間を消費しない
            'o' if i + 1 < chars.len() && chars[i + 1].is_ascii_digit() => {
                i += 1;
                if let Some((_, ni)) = read_u32(&chars, i) { i = ni; }
            }
            'v' if i + 1 < chars.len() && chars[i + 1].is_ascii_digit() => {
                i += 1;
                if let Some((_, ni)) = read_u32(&chars, i) { i = ni; }
            }
            '>' | '<' | '&' => { i += 1; }

            // 休符: 時間を消費する
            'r' => {
                i += 1;
                let (length, dotted, ni) = read_length(&chars, i, last_length);
                last_length = length;
                i = ni;
                let beats = length_to_beats(length, dotted);
                let bpm = if context.is_empty() { local_bpm } else { lookup_tempo(context, time_secs) };
                time_secs += beats * 60.0 / bpm;
            }

            // 音符: シャープを読んで時間を消費する
            'c' | 'd' | 'e' | 'f' | 'g' | 'a' | 'b' => {
                i += 1;
                if i < chars.len() && chars[i] == '+' { i += 1; }
                let (length, dotted, ni) = read_length(&chars, i, last_length);
                last_length = length;
                i = ni;
                let beats = length_to_beats(length, dotted);
                let bpm = if context.is_empty() { local_bpm } else { lookup_tempo(context, time_secs) };
                time_secs += beats * 60.0 / bpm;
            }

            _ => { i += 1; }
        }
    }

    result
}

/// 全チャンネルのテンポイベントをロックステップでシミュレートしてグローバルテンポタイムラインを構築する。
///
/// 実機 MMML エンジンと同様に全チャンネルを同時進行させ、いずれかのチャンネルが `t<n>` コマンドを
/// 処理した時点でグローバルテンポを即時更新する。これにより各チャンネルのテンポ変化の絶対時刻が
/// 正確に計算される。
///
/// t255 はチャンネルローカルな終端マーカーとして扱い、タイムラインには含めない。
fn build_global_tempo_timeline(
    expanded_channels: &[String],
    initial_bpm: f32,
) -> Vec<(f32, f32)> {
    struct ChannelState {
        chars: Vec<char>,
        pos: usize,
        time: f32,
        last_length: u32,
        done: bool,
    }

    let mut channels: Vec<ChannelState> = expanded_channels
        .iter()
        .map(|ch| ChannelState {
            chars: ch.chars().collect(),
            pos: 0,
            time: 0.0,
            last_length: 4,
            done: false,
        })
        .collect();

    let mut global_bpm = initial_bpm;
    let mut timeline: Vec<(f32, f32)> = vec![(0.0, initial_bpm)];

    loop {
        // 最小時刻の未完了チャンネルを選択
        let next_idx = channels
            .iter()
            .enumerate()
            .filter(|(_, ch)| !ch.done)
            .min_by(|(_, a), (_, b)| {
                a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i);

        let idx = match next_idx {
            Some(i) => i,
            None => break,
        };

        // 次の時間消費トークン（音符/休符）まで処理
        let ch = &mut channels[idx];
        let mut advanced = false;

        while ch.pos < ch.chars.len() && !advanced {
            match ch.chars[ch.pos] {
                ' ' | '\t' | '\n' | '\r' => { ch.pos += 1; }

                // テンポコマンド: グローバルテンポを即時更新
                't' if ch.pos + 1 < ch.chars.len() && ch.chars[ch.pos + 1].is_ascii_digit() => {
                    ch.pos += 1;
                    if let Some((n, ni)) = read_u32(&ch.chars, ch.pos) {
                        ch.pos = ni;
                        if n == 255 {
                            ch.done = true;
                            break;
                        }
                        global_bpm = tempo_to_bpm(n);
                        timeline.push((ch.time, global_bpm));
                    }
                }

                // オクターブ・音量: 時間消費なし
                'o' if ch.pos + 1 < ch.chars.len() && ch.chars[ch.pos + 1].is_ascii_digit() => {
                    ch.pos += 1;
                    if let Some((_, ni)) = read_u32(&ch.chars, ch.pos) { ch.pos = ni; }
                }
                'v' if ch.pos + 1 < ch.chars.len() && ch.chars[ch.pos + 1].is_ascii_digit() => {
                    ch.pos += 1;
                    if let Some((_, ni)) = read_u32(&ch.chars, ch.pos) { ch.pos = ni; }
                }
                '>' | '<' | '&' => { ch.pos += 1; }

                // 休符: 時間を進める
                'r' => {
                    ch.pos += 1;
                    let (length, dotted, ni) = read_length(&ch.chars, ch.pos, ch.last_length);
                    ch.last_length = length;
                    ch.pos = ni;
                    let beats = length_to_beats(length, dotted);
                    ch.time += beats * 60.0 / global_bpm;
                    advanced = true;
                }

                // 音符: 時間を進める
                'c' | 'd' | 'e' | 'f' | 'g' | 'a' | 'b' => {
                    ch.pos += 1;
                    if ch.pos < ch.chars.len() && ch.chars[ch.pos] == '+' { ch.pos += 1; }
                    let (length, dotted, ni) = read_length(&ch.chars, ch.pos, ch.last_length);
                    ch.last_length = length;
                    ch.pos = ni;
                    let beats = length_to_beats(length, dotted);
                    ch.time += beats * 60.0 / global_bpm;
                    advanced = true;
                }

                _ => { ch.pos += 1; }
            }
        }

        if ch.pos >= ch.chars.len() {
            ch.done = true;
        }
    }

    // 時刻順にソート・重複除去
    timeline.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut deduped: Vec<(f32, f32)> = Vec::with_capacity(timeline.len());
    for (t, bpm) in timeline {
        if let Some(last) = deduped.last_mut() {
            if (last.0 - t).abs() < 1e-6 {
                last.1 = bpm;
                continue;
            }
        }
        deduped.push((t, bpm));
    }
    deduped
}

/// グローバルテンポタイムラインから指定時刻の BPM を返す。
///
/// タイムラインは昇順ソート済みであること。
#[inline]
fn lookup_tempo(timeline: &[(f32, f32)], time_secs: f32) -> f32 {
    let mut bpm = timeline[0].1;
    for &(t, b) in timeline {
        if t <= time_secs + 1e-9 {
            bpm = b;
        } else {
            break;
        }
    }
    bpm
}

// ─────────────────────────────────────────────────────────
// ステップ 5: 音符テキストの解析
// ─────────────────────────────────────────────────────────

/// 音符テキストの解析状態
struct ParseState {
    octave: u8,          // 1-5 (デフォルト: 3)
    volume: u8,          // 0-8 (デフォルト: 8 = 50% デューティ)
    last_length: u32,    // 最後に明示された長さ (デフォルト: 4 = 四分音符)
    time_secs: f32,      // 現在の時刻 [秒]
    local_bpm: Option<f32>, // t255 遭遇後のチャンネルローカル BPM
}

impl ParseState {
    fn new() -> Self {
        Self {
            octave: 3,
            volume: 8,
            last_length: 4,
            time_secs: 0.0,
            local_bpm: None,
        }
    }

    /// 現在の BPM を返す: t255 以降はローカル BPM、それ以外はグローバルタイムライン
    fn current_bpm(&self, timeline: &[(f32, f32)]) -> f32 {
        self.local_bpm.unwrap_or_else(|| lookup_tempo(timeline, self.time_secs))
    }

    /// 現在のグローバルテンポを参照して拍数を秒に変換
    fn beats_to_secs(&self, beats: f32, timeline: &[(f32, f32)]) -> f32 {
        beats * 60.0 / self.current_bpm(timeline)
    }

    /// 時刻を beats 分だけ進める
    fn advance(&mut self, beats: f32, timeline: &[(f32, f32)]) {
        self.time_secs += self.beats_to_secs(beats, timeline);
    }
}

/// チャンネル MML テキストを解析して SongEvent のリストを生成する。
///
/// テンポはグローバルタイムラインから参照する。チャンネル内の `t<n>` は
/// 既にタイムラインに反映済みなので無視する。
fn parse_channel(
    text: &str,
    vchannel: u8,
    tempo_timeline: &[(f32, f32)],
    instrument: Instrument,
) -> Vec<SongEvent> {
    let mut events: Vec<SongEvent> = Vec::new();
    let mut state = ParseState::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    // タイ処理用
    let mut tie_pending = false;
    let mut last_event_idx: Option<usize> = None;
    let mut tie_start_secs: f32 = 0.0;
    let mut tie_total_secs: f32 = 0.0;

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

        // ─── テンポコマンド ────────────────────────────────────
        // 通常の t コマンド: グローバルタイムラインに反映済みなのでスキップ。
        // t255: チャンネルローカルな終端テンポとして保持し、以降の時間計算に使用。
        if c == 't' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
            i += 1;
            if let Some((n, ni)) = read_u32(&chars, i) {
                if n == 255 {
                    state.local_bpm = Some(tempo_to_bpm(255));
                }
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
            if state.octave < 5 { state.octave += 1; }
            i += 1;
            continue;
        }
        if c == '<' {
            if state.octave > 1 { state.octave -= 1; }
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
            state.advance(beats, tempo_timeline);
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
            if i < chars.len() && chars[i] == '+' {
                semitone += 1;
                i += 1;
            }
            semitone %= 12;

            let (length, dotted, ni) = read_length(&chars, i, state.last_length);
            state.last_length = length;
            i = ni;

            let duration_beats = length_to_beats(length, dotted);
            let duration_secs = state.beats_to_secs(duration_beats, tempo_timeline);
            let freq = mmml_freq(semitone, state.octave);
            let vol = if state.volume == 0 { 0.0 } else { state.volume as f32 / 8.0 };

            // CH4 では note semitone (0-11) が直接 sample_id (buffer1) に相当する。
            // μMML: c=1(bwoop), c+=2(beep), d=3(kick), d+=4(snare), e=5(hi-hat)
            let drum_type = if instrument == Instrument::Percussion {
                Some(DrumType::from_mmml_sample_id((semitone + 1) as u8))
            } else {
                None
            };

            if tie_pending {
                if let Some(idx) = last_event_idx {
                    tie_total_secs += duration_secs;
                    let gate_close_secs = tie_start_secs + tie_total_secs * default_gate;
                    events[idx].gate_close_secs = gate_close_secs.max(events[idx].gate_close_secs);
                }
                tie_pending = false;
            } else {
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
                    drum_type,
                });
            }

            state.advance(duration_beats, tempo_timeline);
            continue;
        }

        // ─── その他はスキップ ────────────────────────────────
        i += 1;
    }

    events
}

// ─────────────────────────────────────────────────────────
// グローバルテンポの初期値を検出するヘルパー
// ─────────────────────────────────────────────────────────

/// 音符/休符より前に現れるテンポコマンドを探す (チャンネル初期テンポ)
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
            if NOTE_CHARS.contains(&c) {
                break;
            }
            if c == 't' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                i += 1;
                if let Some((n, _)) = read_u32(&chars, i) {
                    // t255 はチャンネル終端マーカーなのでテンポとして扱わない
                    if n > 0 && n != 255 {
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
                    if n > 0 && n != 255 {
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

    // 4. 初期テンポを決定
    let initial_bpm = find_initial_tempo(&sections)
        .or_else(|| find_first_tempo(&sections))
        .unwrap_or(120.0);

    // 5. 各チャンネルのループ+マクロを展開
    let expanded_channels: Vec<String> = channel_sections
        .iter()
        .map(|&ch| expand(ch, &macro_sections, 0))
        .collect();

    // 6. グローバルテンポタイムラインを構築
    //    いずれかのチャンネルの t コマンドが全チャンネルのテンポを変更する
    let global_timeline = build_global_tempo_timeline(&expanded_channels, initial_bpm);

    // 7. 各チャンネルを解析 (グローバルタイムラインを参照)
    let mut all_events = Vec::new();

    for (ch_idx, expanded) in expanded_channels.iter().enumerate() {
        let vchannel = (ch_idx as u8) + 1; // 1-4
        let instrument = if ch_idx == 3 {
            Instrument::Percussion
        } else {
            Instrument::Square
        };

        let events = parse_channel(expanded, vchannel, &global_timeline, instrument);
        all_events.extend(events);
    }

    // 8. 時刻順にソート
    all_events.sort_by(|a, b| {
        a.time_secs
            .partial_cmp(&b.time_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 9. 全体の長さを計算
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
        source_tempo_bpm: initial_bpm,
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

    #[test]
    fn test_global_tempo_synchronizes_channels() {
        // CH1 が t52 を設定 → CH2 も同じテンポで動作すること
        // CH1: t52 c4 (quarter at ~121bpm ≈ 0.496s)
        // CH2:     c4 (should also be at ~121bpm ≈ 0.496s)
        let mmml = "@ t52 o1 c4\n@ o1 c4\n@\n@";
        let seq = parse_mmml_file(mmml).unwrap();

        let ch1_events: Vec<_> = seq.events.iter().filter(|e| e.vchannel == 1).collect();
        let ch2_events: Vec<_> = seq.events.iter().filter(|e| e.vchannel == 2).collect();

        assert_eq!(ch1_events.len(), 1);
        assert_eq!(ch2_events.len(), 1);

        // CH2 のゲートクローズが CH1 と同じテンポ基準であること
        let ch1_end = ch1_events[0].gate_close_secs;
        let ch2_end = ch2_events[0].gate_close_secs;
        assert!(
            (ch1_end - ch2_end).abs() < 0.01,
            "グローバルテンポで CH1/CH2 が同期すること: ch1={:.4} ch2={:.4}",
            ch1_end, ch2_end
        );
    }
}
