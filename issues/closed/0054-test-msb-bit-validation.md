# prop_violations.rs に MSB ビット検証を追加する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/add-msb-bit-validation-pbt

## 目的

64 ビットペイロード長フィールドの MSB=1 フレームが `FrameDecoder` で拒否されることを PBT で検証する（RFC 6455 Section 5.2）。

## 優先度根拠

Medium。MSB=1 は負の長さ解釈につながり、usize 変換前に拒否すべき。

## 現状

- `src/websocket_frame.rs:214-218`: MSB 検証あり
- `pbt/tests/prop_violations.rs`: 該当テストなし

## 設計方針

手動構築フレーム（FIN + opcode、127 長形式、MSB=1 の 8 バイト長）を strategy で生成し、`decode()` が `Err` になることを検証。単体テストではなく PBT（意図的エラーパスだが入力の一部を生成するため proptest で十分）。

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] 64 ビットペイロード長 MSB=1 フレームの拒否を PBT で検証する
  - @<GitHub ハンドル>
```

## 完了条件

- MSB=1 のケースが `prop_violations.rs` にある
- `cargo test -p shiguredo_websocket --test prop_violations` がパスする

## 解決方法

`pbt/tests/prop_violations.rs` に `prop_payload_length_64bit_msb_set_rejected` を追加した。

任意の opcode (`Text` / `Binary` / `Close` / `Ping` / `Pong`) と長さフィールドの後続 7 バイトを proptest で生成し、最上位バイトの MSB を強制的に 1 にした 10 バイトのフレームを構築する。`FrameDecoder::feed` → `decode` が `Err` (`ProtocolViolation` 相当) を返すことを検証する。

RFC 6455 Section 5.2 の MSB=0 制約は `src/websocket_frame.rs:214-218` で実装済みで、本テストはそのオフバイワン抑止と仕様準拠の継続的な検証となる。`cargo test -p pbt --test prop_violations` 11 件 (新規 1 + 既存 10) 全件パス。
