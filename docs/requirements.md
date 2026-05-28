# Lumen VIA Beeper Simulator — Requirements Definition v0.1

## 1. プロジェクト概要

### 1.1 名称

**Lumen VIA Beeper Simulator**

略称: **LVB-Sim**

### 1.2 目的

Lumen65 標準音源として想定する、

- W65C22S VIA による 2ch ハードウェア矩形波出力
- 高速アルペジオによる疑似 3〜4ch 表現
- 圧電サウンダによる実機音響特性

をPC上で再現・検証するためのシミュレータを開発する。

本シミュレータは、将来的な Lumen65 実機実装に先立ち、以下を評価する。

- VIA 2ch beeper の音楽的表現力
- 高速アルペジオによる疑似多声表現
- μMML 風データのサブセット再生可能性
- 圧電サウンダ特性を考慮した実際の聞こえ方
- CPU負荷と音楽表現のトレードオフ

---

## 2. 背景

Lumen Pulse Engine (LPE) v2.0 では、μMML相当の表現力を目標として、

- 3ch パルス波
- 1bit サンプラー
- トランスペアレントDMA
- CPUリソース非消費

を実現する拡張音源が検討されている。

一方で、Lumen65 の標準音源としては、部品点数・実装難度・消費電力の観点から、より簡素な音源が望ましい。

W65C22S VIA には Timer 1 / Timer 2 / PB7 / CB2 等を利用した矩形波・パルス生成機能があり、2ch程度の beeper 音源であれば外部ロジックをほぼ追加せず実装可能である。

本シミュレータでは、この **VIA 2ch beeper** を基本音源とし、高速アルペジオにより疑似的に 3〜4ch 相当の表現を行う方式を検証する。

---

## 3. 目標

### 3.1 主目標

VIA 2ch beeper によって、以下のような音楽表現が可能か検証する。

- 2ch 矩形波 BGM
- 高速アルペジオによる疑似コード表現
- 疑似 3ch / 疑似 4ch 表現
- 簡易ドラム、クリック音、効果音
- μMML 風シーケンスのサブセット再生

### 3.2 副目標

- 実際の圧電ブザーで鳴らした場合の音量・帯域感を近似する
- 1MHz / 500kHz 65C02 相当でのCPU負荷を推定する
- LPE-DMA拡張音源との役割分担を明確にする
- 将来の実機プレイヤー実装に使えるデータ構造を検討する

---

## 4. 想定ハードウェア

### 4.1 CPU

| 項目         | 値                                            |
| ------------ | --------------------------------------------- |
| CPU          | W65C02S                                       |
| 動作クロック | 500kHz / 1MHz / 2MHz                          |
| 主用途       | 曲データ解釈、VIAレジスタ更新、アルペジオ制御 |

### 4.2 VIA

| 項目     | 値                             |
| -------- | ------------------------------ |
| VIA      | W65C22S                        |
| CH-A     | Timer 1 / PB7                  |
| CH-B     | Timer 2 / CB2 またはポート制御 |
| 出力形式 | 矩形波 / パルス波              |
| 音程制御 | VIA Timer ラッチ値の書き換え   |

### 4.3 音声出力

| 項目         | 値                    |
| ------------ | --------------------- |
| 出力デバイス | 圧電サウンダ          |
| 想定型番     | TDK PS1720P02 相当    |
| 駆動方式     | 直接駆動または簡易BTL |
| 電源電圧     | 3.3V / 5V             |
| 実用帯域     | 約1kHz〜4kHzを重視    |
| 共振周波数   | 約2kHz                |

---

## 5. 音源仕様

### 5.1 実チャンネル

#### CH-A

| 項目       | 内容                         |
| ---------- | ---------------------------- |
| 出力元     | VIA Timer 1 / PB7            |
| 波形       | 50%矩形波                    |
| 主用途     | メロディ、アルペジオ、装飾音 |
| 周波数範囲 | 約100Hz〜8kHz                |
| 推奨範囲   | 約500Hz〜4kHz                |

#### CH-B

| 項目       | 内容                                 |
| ---------- | ------------------------------------ |
| 出力元     | VIA Timer 2 / CB2 または VIA制御出力 |
| 波形       | 矩形波 / パルス                      |
| 主用途     | ベース、対旋律、効果音               |
| 周波数範囲 | 約100Hz〜8kHz                        |
| 推奨範囲   | 約300Hz〜3kHz                        |

---

## 6. 疑似チャンネル仕様

### 6.1 疑似4ch構成

実際のハードウェア出力は2chとし、CPUが高速に音程を切り替えることで疑似的に4chを表現する。

| 論理CH | 割当例                        | 備考                |
| ------ | ----------------------------- | ------------------- |
| VCH1   | CH-A 主旋律                   | 優先度高            |
| VCH2   | CH-B ベース                   | 優先度高            |
| VCH3   | CH-A/CH-B アルペジオ内挿      | 和音・副旋律        |
| VCH4   | 短音パーカッション / クリック | μMML CH4 の簡易代替 |

### 6.2 アルペジオ方式

一定周期で実チャンネルの周波数を切り替える。

例:

```text
Arp Tick 0:
  CH-A = VCH1 Lead
  CH-B = VCH2 Bass

Arp Tick 1:
  CH-A = VCH3 Chord note 1
  CH-B = VCH2 Bass

Arp Tick 2:
  CH-A = VCH1 Lead
  CH-B = VCH4 Percussive click

Arp Tick 3:
  CH-A = VCH3 Chord note 2
  CH-B = VCH2 Bass
````

### 6.3 アルペジオ更新レート

| レート | 用途      | CPU負荷 | 聴感                 |
| ------ | --------- | ------- | -------------------- |
| 60Hz   | 低負荷BGM | 低      | 分散和音感が強い     |
| 120Hz  | 標準      | 低〜中  | 疑似多声として自然   |
| 240Hz  | 高品質    | 中      | 疑似同時発音感が増す |
| 480Hz  | 実験的    | 中〜高  | 滑らかだがCPU負荷増  |
| 1kHz   | 非推奨    | 高      | 実装負荷が高い       |

推奨値: **120Hz または 240Hz**

---

## 7. μMMLサブセット互換

### 7.1 目的

μMMLの完全互換ではなく、Lumen65標準音源向けに変換可能なサブセットを定義する。

### 7.2 対応対象

| μMML要素        | 対応方針                                 |
| --------------- | ---------------------------------------- |
| CH1 パルス波    | CH-A または VCH1                         |
| CH2 パルス波    | CH-B または VCH2                         |
| CH3 パルス波    | 高速アルペジオ内の VCH3                  |
| CH4 1bit sample | クリック音 / 短音ノイズ / 音程急変で近似 |
| note            | 対応                                     |
| rest            | 対応                                     |
| tempo           | 対応                                     |
| octave          | 対応                                     |
| length          | 対応                                     |
| volume          | ON/OFF密度、ゲート長、優先度で近似       |
| duty            | 原則非対応、将来拡張                     |
| envelope        | 簡易対応                                 |
| sample playback | 簡易近似のみ                             |

### 7.3 非目標

以下は本シミュレータ v0.1 では非目標とする。

* μMML完全互換
* 1bit PCMサンプルの完全再現
* 3ch以上の真の同時発音
* 任意デューティ比パルス波
* 高忠実度PCM音源

---

## 8. 音響シミュレーション

### 8.1 基本方針

シミュレータは、理想矩形波をそのまま出力するだけでなく、圧電サウンダの特性を簡易的に反映する。

### 8.2 圧電サウンダモデル

対象: TDK PS1720P02 相当

| 項目       | モデル                         |
| ---------- | ------------------------------ |
| 共振周波数 | 約2kHz                         |
| 実用帯域   | 約1kHz〜4kHz                   |
| 低域       | 大きく減衰                     |
| 高域       | 容量性負荷・機械特性により減衰 |
| 音量       | 周波数依存                     |
| 出力       | モノラル                       |

### 8.3 フィルタモデル

v0.1では以下の簡易モデルを実装する。

```text
Input waveform
  ↓
DC blocking high-pass filter
  ↓
Piezo resonance band-pass approximation
  ↓
Optional soft clipping
  ↓
Output WAV / realtime audio
```

### 8.4 推奨フィルタ

| 処理           | 目安         |
| -------------- | ------------ |
| High-pass      | 300Hz〜800Hz |
| Resonance peak | 2kHz付近     |
| Band limit     | 6kHz〜10kHz  |
| Low-pass       | 8kHz〜12kHz  |

### 8.5 駆動電圧モデル

| 駆動              | 出力振幅モデル |
| ----------------- | -------------- |
| 3.3V single-ended | 小             |
| 3.3V BTL          | 中             |
| 5V single-ended   | 中             |
| 5V BTL            | 大             |

シミュレータでは `drive_mode` として選択可能にする。

---

## 9. シミュレータ機能要件

### 9.1 基本再生

シミュレータは以下を実装する。

* 2ch VIA beeper 音源生成
* 周波数ラッチ値による音程生成
* CH-A / CH-B の矩形波生成
* 高速アルペジオスケジューラ
* 論理4chから実2chへの割当
* WAVファイル書き出し
* リアルタイム再生

### 9.2 音源パラメータ

各実チャンネルは以下の状態を持つ。

```text
channel {
  enabled: bool
  frequency_hz: float
  phase: float
  duty: float = 0.5
  gate: bool
  volume: float
}
```

### 9.3 論理チャンネル

各論理チャンネルは以下の状態を持つ。

```text
virtual_channel {
  enabled: bool
  note: int
  frequency_hz: float
  volume: int
  priority: int
  instrument: enum
  gate_length: float
}
```

### 9.4 アルペジオスケジューラ

以下のモードを持つ。

| モード     | 内容                               |
| ---------- | ---------------------------------- |
| OFF        | 実2chのみ                          |
| Pseudo3    | CH-Aに補助音を混ぜる               |
| Pseudo4    | CH-A/CH-B双方で時分割              |
| BassLock   | CH-Bをベース固定、CH-Aで疑似多声   |
| MelodyLock | CH-Aをメロディ優先、CH-Bで疑似多声 |

推奨初期モード: **BassLock**

---

## 10. CPU負荷推定機能

### 10.1 目的

シミュレータ上で、Lumen65実機におけるCPU負荷を概算する。

### 10.2 想定CPU

| CPUクロック | 用途         |
| ----------- | ------------ |
| 500kHz      | 省電力モード |
| 1MHz        | 標準         |
| 2MHz        | 高速モード   |

### 10.3 推定対象処理

* テンポ管理
* 曲データ解釈
* アルペジオ更新
* VIA Timer ラッチ書き換え
* ゲート制御
* 効果音優先制御

### 10.4 負荷モデル

以下のようなパラメータを持つ。

```text
cycles_per_music_tick
cycles_per_arp_update
cycles_per_via_write
cycles_per_effect_trigger
cpu_clock_hz
```

負荷率は以下で算出する。

```text
cpu_load = used_cycles_per_second / cpu_clock_hz
```

### 10.5 目標負荷

| モード        | 目標負荷 @1MHz |
| ------------- | -------------- |
| 2ch simple    | 5%未満         |
| 2ch BGM       | 5〜10%         |
| Pseudo3       | 10〜15%        |
| Pseudo4 120Hz | 10〜20%        |
| Pseudo4 240Hz | 15〜30%        |
| Heavy SFX     | 30%以下        |

---

## 11. 入力データ仕様

### 11.1 対応入力形式

| 形式           | 優先度 | 内容               |
| -------------- | -----: | ------------------ |
| YAML中間形式   |     高 | v0.1の内部標準形式 |
| mmml / μMML    |     高 | ベンチマーク入力   |
| MML風テキスト  |     中 | 将来の簡易作曲用   |
| JSON中間形式   |     中 | ツール連携用       |
| Raw event list |     低 | デバッグ用         |

### 11.2 mmml入力

mmml入力は、まずベンチマーク用途として対応する。

```bash
lvb-sim input.mmml --out output.wav
````

内部では、mmmlを直接レンダリングせず、一度LVB中間表現へ変換する。

```text
mmml
  ↓ parse
LVB IR
  ↓ arrange
VIA 2ch schedule
  ↓ render
audio
```

---

## 12. 出力仕様

### 12.1 音声出力

| 出力           | 内容                          |
| -------------- | ----------------------------- |
| Realtime Audio | リアルタイム再生              |
| WAV            | 44.1kHz / 48kHz               |
| Raw 1bit Debug | 内部波形確認用                |
| CSV            | 周波数・VIAレジスタ・負荷ログ |

### 12.2 可視化

以下を表示する。

* CH-A 波形
* CH-B 波形
* MIX波形
* 圧電モデル通過後波形
* 周波数スペクトラム
* アルペジオ割当タイムライン
* CPU負荷グラフ
* VIAレジスタ更新ログ

---

## 13. UI要件

### 13.1 最小CLI

v0.1ではCLIを必須とする。

```bash
lvb-sim input.yaml --out output.wav
```

オプション例:

```bash
lvb-sim input.yaml \
  --cpu 1000000 \
  --sample-rate 48000 \
  --arp-rate 240 \
  --mode BassLock \
  --piezo PS1720P02 \
  --drive 3v3-btl \
  --out demo.wav
```

### 13.2 将来GUI

v0.2以降でGUIを検討する。

* トラックエディタ
* アルペジオパターン表示
* 波形表示
* 圧電モデルON/OFF切替
* 実機レジスタログ表示

---

## 14. 実装方針

### 14.1 推奨実装言語

候補:

* Python
* TypeScript
* Rust
* C++

v0.1では実験速度を優先し、**Python** を推奨する。

### 14.2 Python構成案

```text
lvb_sim/
  __init__.py
  main.py
  sequence.py
  via.py
  beeper.py
  arpeggiator.py
  piezo_model.py
  renderer.py
  cpu_load.py
  visualizer.py
examples/
  simple_2ch.yaml
  pseudo4_demo.yaml
  basslock_demo.yaml
```

### 14.3 主要クラス

```text
VIAEmulator
  - timer1_latch
  - timer2_latch
  - pb7_output
  - cb2_output

BeeperChannel
  - frequency
  - phase
  - duty
  - enabled

VirtualChannel
  - note
  - volume
  - priority
  - instrument

Arpeggiator
  - mode
  - rate_hz
  - assign_virtual_to_physical()

PiezoModel
  - apply_filter()
  - drive_mode

Renderer
  - render_to_buffer()
  - write_wav()
```

---

## 15. 実機反映要件

シミュレータは、将来的な実機プレイヤーへ変換可能な情報を出力する。

### 15.1 VIAレジスタログ

以下の形式で出力する。

```csv
time_us, reg, value, description
0, T1CL, 0x34, CH-A frequency low
0, T1CH, 0x12, CH-A frequency high
4166, T2CL, 0x88, CH-B frequency low
4166, T2CH, 0x09, CH-B frequency high
```

### 15.2 実機用中間データ

```text
tick:
  wait_cycles
  ch_a_timer_latch
  ch_b_timer_latch
  gate_flags
  arp_slot
```

---

## 16. 品質要件

### 16.1 音楽的品質

* 2chとして自然に聞こえること
* 120Hz以上のアルペジオで疑似和音感が得られること
* ベースラインが破綻しないこと
* 圧電モデルON時に低域が過度に期待されないこと
* 2kHz近辺で音量感が増すこと

### 16.2 技術的品質

* 同じ入力から決定的に同じWAVが出力されること
* サンプルレート依存の音程誤差が小さいこと
* VIAタイマー値と周波数の変換が明示されていること
* CPU負荷推定がログとして確認できること

---

## 17. 制約条件

### 17.1 ハードウェア制約

* 標準音源では外部音源ICを使用しない
* 標準音源ではDMAを使用しない
* 標準音源では原則としてVIAのみで2ch生成する
* 圧電サウンダ直接駆動を前提とする

### 17.2 ソフトウェア制約

* 1MHz 65C02で実行可能な処理量に収める
* アルペジオ更新は原則 240Hz 以下を推奨
* 曲データ解釈と描画・ゲーム処理が共存できる負荷を目指す

---

## 18. 非目標

本プロジェクトでは以下を目標としない。

* PCM音源としての高音質化
* PSG完全互換
* SID / AY-3-8910 / SN76489 互換
* μMML完全互換
* LPE-DMAの代替
* 高忠実度スピーカー再生
* ステレオ出力

---

## 19. LPE-DMAとの関係

### 19.1 位置づけ

| 項目       | LVB-Sim / VIA Beeper | LPE-DMA          |
| ---------- | -------------------- | ---------------- |
| 目的       | 標準音源             | 拡張音源         |
| 外部IC     | ほぼ不要             | 多い             |
| CPU負荷    | 低〜中               | ほぼゼロ         |
| 実ch       | 2ch                  | 4ch相当          |
| μMML互換   | サブセット           | 高互換を目標     |
| サンプラー | 簡易クリック         | 1bitサンプラー   |
| 実装難度   | 低                   | 高               |
| 教育価値   | 高                   | 非常に高いが複雑 |

### 19.2 併用方針

Lumen65本体には VIA Beeper を標準搭載し、拡張スロットまたは外部ボードとして LPE-DMA を追加可能とする。

---

## 20. 開発フェーズ

### Phase 1: 基本2ch beeper

* CH-A / CH-B の矩形波生成
* WAV出力
* 簡易YAML入力
* 周波数指定再生

完了条件:

* 2ch矩形波のWAVを書き出せること

### Phase 2: 高速アルペジオ

* 疑似3ch / 疑似4ch
* BassLock / MelodyLock
* アルペジオ更新レート指定

完了条件:

* 120Hz / 240Hz の疑似4chデモが鳴ること

### Phase 3: 圧電サウンダモデル

* High-pass / band-pass / resonance approximation
* drive_mode対応
* 圧電モデルON/OFF比較

完了条件:

* PS1720P02風の帯域感を近似できること

### Phase 4: CPU負荷推定

* VIAレジスタ更新ログ
* cycle見積もり
* CPU負荷グラフ

完了条件:

* 1MHz 65C02想定の負荷率を出力できること

### Phase 5: μMMLサブセット変換

* MML風入力
* note/rest/tempo/octave/length対応
* CH割当変換

完了条件:

* 簡単なμMML風曲を疑似4chで再生できること

---

## 21. リスクと対策

| リスク                            | 影響                   | 対策                                      |
| --------------------------------- | ---------------------- | ----------------------------------------- |
| 疑似4chが分散和音にしか聞こえない | 同時発音感が弱い       | 120Hz/240Hz/480Hzを比較                   |
| 低音が圧電ブザーで鳴らない        | ベースが弱い           | ベースを高めに配置、倍音重視              |
| CPU負荷が高い                     | ゲーム等と共存しにくい | BassLock / 120Hz標準化                    |
| CH4ドラム再現が弱い               | μMML感が減る           | クリック、短パルス、音程急変で近似        |
| CB2の実機制約が大きい             | 2ch目が期待通り出ない  | 実機VIA仕様を確認し、PBポート制御案も用意 |
| 圧電モデルが不正確                | 実機との差が出る       | 実測録音による補正を将来対応              |

---

## 22. 成功基準

v0.1の成功基準は以下とする。

* 2ch VIA beeper の基本波形を生成できる
* 疑似4chアルペジオのデモ曲が作れる
* 圧電サウンダ風フィルタを通した音が確認できる
* 120Hz / 240Hz の聴感差を比較できる
* 1MHz 65C02想定のCPU負荷率を概算できる
* Lumen65標準音源として採用可能か判断できる

---

## 23. まとめ

Lumen VIA Beeper は、LPE-DMAほど高機能ではないが、Lumen65標準音源として非常に現実的である。

特に、

* 部品点数が増えない
* VIAの標準機能を活用できる
* CPU負荷が比較的低い
* 高速アルペジオで疑似多声が可能
* 圧電サウンダとの相性が良い

という利点がある。

本シミュレータにより、Lumen65における「標準音源」と「拡張音源」の境界を明確化し、実機実装前に音楽的・技術的な成立性を検証する。

はい、それはかなり良いです。
**作曲スキルがなくても評価できるベンチマーク曲セット**があると、シミュレータの価値が一気に上がります。

要件定義書には、次の章を追加するとよさそうです。
既存の LPE 側でも μMML を参考実装として位置づけているので、VIA Beeper版でも「μMML/mmmlを入力ベンチマークとして使う」方針は自然です。

````markdown
## 24. μMML / mmml ベンチマーク入力対応

### 24.1 目的

本シミュレータでは、作曲済みの μMML / mmml 楽曲データをベンチマーク入力として利用できるようにする。

これにより、開発者が独自に作曲しなくても、以下を評価できる。

- VIA 2ch beeper + 高速アルペジオ疑似4ch の表現力
- μMML系データからの変換可能性
- 原曲に対する再現度
- 圧電サウンダでの聴感変化
- アルペジオ割当アルゴリズムの良し悪し
- CPU負荷と音楽品質のバランス

### 24.2 ベンチマーク対象

優先的に対応する入力データは以下とする。

| 種別 | 用途 |
|---|---|
| Protodome氏の μMML / mmml サンプル曲 | 基準ベンチマーク |
| 短いデモ曲 | 回帰テスト |
| ドラム入り曲 | CH4近似評価 |
| 和音・アルペジオ主体の曲 | 疑似多声評価 |
| 高速フレーズ曲 | VIAレジスタ更新負荷評価 |

### 24.3 入力方針

v0.1では、μMML / mmml の完全パーサを実装せず、まずは以下の2段階方式とする。

```text
.mmml / μMML source
        ↓
  parser / converter
        ↓
LVB intermediate representation
        ↓
VIA 2ch + pseudo4 renderer
        ↓
WAV / realtime audio / analysis log
````

### 24.4 中間表現への変換

μMML / mmml の各チャンネルを、Lumen VIA Beeper の仮想チャンネルへ変換する。

| μMML側      | LVB-Sim側 | 変換方針                       |
| ---------- | -------- | -------------------------- |
| CH1        | VCH1     | 主旋律として優先                   |
| CH2        | VCH2     | ベースまたは副旋律                  |
| CH3        | VCH3     | 高速アルペジオ内で再現                |
| CH4 sample | VCH4     | クリック / 短パルス / 擬似パーカッションへ変換 |

### 24.5 変換モード

複数の変換モードを用意し、同じmmmlファイルに対して聴感比較できるようにする。

| モード                   | 内容                          |
| --------------------- | --------------------------- |
| `direct2`             | CH1→CH-A、CH2→CH-Bのみ。最小変換    |
| `basslock`            | CH-Bをベース優先、CH-Aでメロディと和音を時分割 |
| `melodylock`          | CH-Aを主旋律優先、CH-Bでベースと装飾音を時分割 |
| `pseudo4`             | CH1〜CH4を2chへ積極的に時分割配置       |
| `percussion-priority` | CH4のクリック/ドラム近似を優先           |
| `low-cpu`             | アルペジオ更新頻度を抑え、CPU負荷を優先       |

### 24.6 ベンチマーク実行例

CLIでは以下のように実行できるものとする。

```bash
lvb-sim benchmark protodome_sample.mmml \
  --mode basslock \
  --arp-rate 240 \
  --piezo PS1720P02 \
  --drive 3v3-btl \
  --cpu 1000000 \
  --out protodome_basslock.wav
```

複数モードを一括比較するコマンドも用意する。

```bash
lvb-sim benchmark protodome_sample.mmml \
  --compare direct2,basslock,melodylock,pseudo4 \
  --arp-rate 120,240,480 \
  --piezo PS1720P02 \
  --out-dir benchmark_results/
```

### 24.7 出力されるベンチマーク結果

ベンチマーク実行時には以下を出力する。

```text
benchmark_results/
  original_info.json
  direct2_240hz.wav
  basslock_240hz.wav
  melodylock_240hz.wav
  pseudo4_240hz.wav
  analysis.csv
  cpu_load.csv
  via_register_log.csv
  summary.md
```

### 24.8 評価指標

ベンチマークでは以下の指標を出力する。

| 指標                        | 内容                  |
| ------------------------- | ------------------- |
| `note_preservation_rate`  | 元データの音符をどれだけ鳴らせたか   |
| `channel_drop_rate`       | 捨てられた論理チャンネルイベントの割合 |
| `priority_loss`           | 優先度の高い音が落ちた割合       |
| `arp_switch_rate`         | 1秒あたりの音程切替回数        |
| `via_write_rate`          | 1秒あたりのVIAレジスタ更新回数   |
| `estimated_cpu_load`      | 65C02想定CPU負荷        |
| `piezo_band_energy`       | 圧電サウンダ実用帯域内のエネルギー   |
| `low_band_loss`           | 低音成分の減衰量            |
| `percussion_approx_score` | CH4近似の成立度           |

### 24.9 Protodome mmml互換レベル

v0.1では完全互換を目標にせず、以下の段階的対応とする。

| レベル     | 内容                                        |
| ------- | ----------------------------------------- |
| Level 0 | mmmlファイルを読み込み、メタ情報を表示                     |
| Level 1 | note / rest / octave / length / tempo を変換 |
| Level 2 | 3ch pulse相当をVCH1〜VCH3へ変換                  |
| Level 3 | CH4 sampleイベントをクリック/短パルスへ変換               |
| Level 4 | volume / duty / envelope を近似              |
| Level 5 | μMML/mmmlサブセットとして実用的に再生可能                 |

v0.1の目標は **Level 2〜3** とする。
v0.2以降で Level 4〜5 を目指す。

### 24.10 ライセンス・配布上の注意

Protodome氏のmmmlファイルを同梱する場合は、元データのライセンスを確認する。

シミュレータ本体では以下の方針を取る。

* ライセンスが明確なサンプルのみ同梱する
* 不明な場合は同梱せず、ユーザーがローカルに配置したmmmlを読み込む
* ベンチマーク用URLまたはパス指定に対応する
* 変換後WAVの再配布可否は元データのライセンスに従う

### 24.11 ベンチマーク曲管理

ベンチマーク曲は以下のようなメタデータで管理する。

```yaml
id: protodome_demo_001
title: example title
author: PROTODOME
source_file: path/to/example.mmml
license: unknown
channels:
  ch1: pulse
  ch2: pulse
  ch3: pulse
  ch4: sample
tags:
  - arpeggio
  - percussion
  - high-tempo
  - benchmark
expected_features:
  - lead melody
  - bass line
  - chord arpeggio
  - simple percussion
```

### 24.12 成功基準

Protodome氏のmmmlファイルをベンチマーク入力とした場合、以下を満たすことを成功基準とする。

* mmmlファイルを読み込める
* 少なくともCH1〜CH3の音符情報を抽出できる
* VIA 2ch + 疑似4chへ自動割当できる
* 120Hz / 240Hz / 480Hz の比較WAVを生成できる
* 圧電サウンダモデルON/OFFの比較ができる
* 1MHz 65C02想定のCPU負荷を推定できる
* 変換結果の欠落チャンネル・欠落音符をレポートできる
