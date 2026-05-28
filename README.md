# LVB-Sim — Lumen VIA Beeper Simulator

**LVB-Sim** は、[Lumen65](https://github.com/siska-tech/lumen65) 向けに設計された  
**W65C22S VIA 2ch ビーパー + 高速アルペジオ疑似 4ch** 音源シミュレータです。

実機 Lumen65 への実装前に、VIA beeper の音楽的表現力・CPU 負荷・  
圧電サウンダ特性をPC上で検証することを目的としています。

---

## 特徴

| 機能                    | 詳細                                                           |
| ----------------------- | -------------------------------------------------------------- |
| **2ch 矩形波生成**      | W65C22S Timer 1 / Timer 2 による矩形波を忠実にシミュレート     |
| **疑似 4ch アルペジオ** | 高速時分割 (60〜480 Hz) で最大 4 論理チャンネルを 2ch に多重化 |
| **アルペジオモード**    | Off / Pseudo3 / Pseudo4 / BassLock / MelodyLock                |
| **圧電サウンダモデル**  | TDK PS1720P02 相当の HPF + 共振ピーク + LPF フィルタチェーン   |
| **mmml / μMML 入力**    | Protodome mmml フォーマットのパーサ実装                        |
| **WAV 出力**            | 44.1kHz / 48kHz モノラル WAV 書き出し                          |
| **VIA レジスタログ**    | Timer ラッチ値を CSV で出力（実機移植用）                      |
| **CPU 負荷推定**        | 1MHz / 500kHz / 2MHz W65C02S 想定の負荷率を概算                |
| **ベンチマーク比較**    | 複数モード × 複数アルペジオレートを一括比較                    |

---

## ビルド

Rust toolchain (1.70 以降) が必要です。

```sh
cd lvb-sim
cargo build --release
```

バイナリは `lvb-sim/target/release/lvb-sim` に生成されます。

---

## 使い方

### 基本 (YAML 入力)

```sh
lvb-sim examples/basslock_demo.yaml --out output.wav
```

### オプション指定

```sh
lvb-sim examples/pseudo4_demo.yaml \
  --mode BassLock \
  --arp-rate 240 \
  --cpu 1000000 \
  --sample-rate 48000 \
  --drive 3v3-btl \
  --out demo.wav
```

### mmml 入力

```sh
lvb-sim mysong.mmml --mode BassLock --arp-rate 240 --out mysong.wav
```

### ベンチマーク比較

```sh
# 単一ファイル・複数モード × アルペジオレートを一括出力
lvb-sim benchmark mysong.mmml \
  --compare direct2,basslock,melodylock,pseudo4 \
  --arp-rate 120,240,480 \
  --drive 3v3-btl \
  --out-dir results/
```

---

## CLI オプション一覧

```
USAGE:
    lvb-sim [OPTIONS] [INPUT]
    lvb-sim benchmark [OPTIONS] <INPUT>

ARGS:
    <INPUT>    入力ファイル (.yaml / .mmml)

OPTIONS:
    --out <FILE>           出力 WAV ファイル (省略時: <input>_output.wav)
    --cpu <HZ>             CPU クロック [Hz] (default: 1000000)
    --sample-rate <HZ>     サンプルレート [Hz] (default: 48000)
    --arp-rate <HZ>        アルペジオ更新レート [Hz] (default: 240)
    --mode <MODE>          アルペジオモード (default: BassLock)
                           Off / Pseudo3 / Pseudo4 / BassLock / MelodyLock
    --drive <MODE>         駆動モード (default: 3v3-btl)
                           3v3-single / 3v3-btl / 5v-single / 5v-btl
    --piezo <MODEL>        圧電モデル (現在 PS1720P02 固定)
    --no-piezo             圧電モデルを無効化 (理想矩形波のまま出力)
```

---

## 入力フォーマット

### YAML (LVB 独自形式)

```yaml
title: "Demo"
author: "LVB-Sim"
tempo: 120

tracks:
  - channel: 1          # VCH1: メロディ (優先度高)
    priority: 10
    instrument: square
    events:
      - {note: "C4", length: 4}
      - {note: "E4", length: 4}
      - {note: "G4", length: 2}

  - channel: 2          # VCH2: ベース
    priority: 8
    instrument: square
    events:
      - {note: "C3", length: 1}

  - channel: 4          # VCH4: パーカッション
    priority: 3
    instrument: percussion
    events:
      - {note: "C5", length: 8, gate: 0.05}
      - {rest: true,  length: 8}
```

| フィールド   | 型                       | 説明                          |
| ------------ | ------------------------ | ----------------------------- |
| `channel`    | 1〜4                     | 論理チャンネル番号            |
| `priority`   | 整数                     | 値が大きいほど優先度高        |
| `instrument` | `square` / `percussion`  | 楽器タイプ                    |
| `note`       | `"C4"`, `"A#3"`, `"Bb4"` | 音符名                        |
| `freq`       | float (Hz)               | 周波数直指定 (note の代替)    |
| `rest`       | bool                     | 休符                          |
| `length`     | 1/2/4/8/16/32            | 音符の長さ (4 = 四分音符)     |
| `dotted`     | bool                     | 付点 (長さ × 1.5)             |
| `volume`     | 0〜15                    | 音量 (default: 12)            |
| `gate`       | 0.0〜1.0                 | ゲート長比率 (default: 0.875) |

### mmml / μMML

Protodome氏 の [mmml](https://github.com/protodome/mmml) フォーマットに対応。  
`@` でチャンネルを区切り、最大 4ch (A/B/C/D) を入力できます。

---

## 出力ファイル

| ファイル              | 内容                         |
| --------------------- | ---------------------------- |
| `<name>_output.wav`   | レンダリング済み WAV         |
| `<name>_via_log.csv`  | VIA Timer ラッチ書き込みログ |
| `<name>_cpu_load.csv` | CPU 負荷推定ログ             |

### VIA レジスタログ形式

```csv
time_us,reg,value_hex,value_dec,description
0,T1CL,0x34,52,CH-A 523.3Hz low
0,T1CH,0x00,0,CH-A 523.3Hz high
4166,T2CL,0x88,136,CH-B 261.6Hz low
```

---

## アルペジオモード詳細

| モード       | 挙動                                                |
| ------------ | --------------------------------------------------- |
| `Off`        | 実 2ch のみ使用。VCH1 → CH-A, VCH2 → CH-B           |
| `Pseudo3`    | CH-A に VCH1/VCH3 を時分割挿入                      |
| `Pseudo4`    | CH-A / CH-B 両方を時分割で 4ch 多重化               |
| `BassLock`   | CH-B を VCH2 (ベース) に固定、CH-A で残りを時分割   |
| `MelodyLock` | CH-A を VCH1 (メロディ) に固定、CH-B で残りを時分割 |

推奨モード: **BassLock** (ベースラインを途切れさせず疑似多声を実現)

---

## 圧電サウンダモデル

TDK PS1720P02 相当の特性を 3 段 Biquad フィルタで近似します。

```
矩形波入力
  → High-pass (500 Hz) … DC カット・低域減衰
  → Peaking EQ (2 kHz, +6 dB, Q=2.0) … 共振ピーク
  → Low-pass (10 kHz) … 高域制限
  → tanh ソフトクリッピング
  → 出力
```

`--no-piezo` で無効化し、理想矩形波をそのまま出力することもできます。

---

## プロジェクト構成

```
lvb-sim/
├── Cargo.toml
├── src/
│   ├── main.rs           CLI エントリポイント
│   ├── via.rs            W65C22S VIA エミュレータ
│   ├── beeper.rs         物理チャンネル (矩形波生成)
│   ├── virtual_channel.rs 論理チャンネル (VCH1〜VCH4)
│   ├── arpeggiator.rs    アルペジオスケジューラ
│   ├── piezo_model.rs    圧電サウンダ音響モデル
│   ├── sequence.rs       シーケンスデータ / YAML パーサ
│   ├── mml/
│   │   ├── mod.rs
│   │   └── parser.rs     mmml / μMML パーサ
│   ├── renderer.rs       レンダリングエンジン
│   ├── cpu_load.rs       CPU 負荷推定
│   ├── spectral.rs       スペクトル分析
│   └── benchmark.rs      ベンチマーク比較ランナー
└── examples/
    ├── simple_2ch.yaml   シンプル 2ch デモ
    ├── basslock_demo.yaml BassLock 疑似 4ch デモ
    └── pseudo4_demo.yaml  Pseudo4 モード比較デモ

docs/
└── requirements.md       要件定義書
```

---

## 背景 / 位置づけ

Lumen65 は W65C02S ベースのレトロスタイル 8bit コンピュータです。  
音源として、部品点数を最小化するため **VIA 内蔵タイマー** のみで 2ch 矩形波を生成します。

本シミュレータは実機実装前の検証ツールであり、以下を評価します：

- 高速アルペジオによる疑似多声の聴感
- 120Hz / 240Hz / 480Hz でのアルペジオ品質差
- 1MHz / 500kHz クロックでの CPU 負荷
- 圧電サウンダ通過後の実聴感

詳細は [docs/requirements.md](docs/requirements.md) を参照してください。

---

## ライセンス

MIT License — 詳細は [LICENSE](LICENSE) を参照。

---

## 関連プロジェクト

- [Lumen65](https://github.com/siska-tech/lumen65) — W65C02S ベース 8bit コンピュータ本体
