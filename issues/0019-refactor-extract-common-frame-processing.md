# 0019: クライアント/サーバー間のフレーム処理ロジックの重複を解消する

- Priority: High
- Created: 2026-05-14
- Completed: -
- Model: DeepSeek V4 Flash
- Branch: feature/add-extract-shared-connection-state

ブランチ prefix について: `connection_shared` / `connection_types` という新規モジュールの
**追加**として位置付けるため `feature/add-` を採用する。CLAUDE.md には `refactor`
カテゴリの prefix が定義されていないが、本 issue はリファクタリングそのものを目的としつつ
新規モジュールを増やす作業として `add-` の範疇に入る。ファイル名側の category として
`refactor` を新規採用する（CLAUDE.md は category 文字列を限定していない）。

## 目的

`WebSocketClientConnection` と `WebSocketServerConnection` の間でフレーム処理ロジックが
コピーペーストで重複しており、RFC 違反などのバグ修正時に両方の修正を強制される。
共通の状態 + 振る舞いを `SharedConnectionState` 構造体に切り出し、
マスキング有無などクライアント/サーバーで挙動が分かれる部分だけを `FramePolicy` トレイト
経由で抽象化することで、片方だけ修正してもう片方を忘れるリスクを構造的に排除する。

## 優先度根拠

High。以下の根拠から判断する。

- 重複が広範: 20 を超えるメソッド + フラグメントバッファ + 3 つの定数が完全に重複し、
  単純なコピー一致行数は 400 行を超える。
- バグの取り逃しが発生済み: `close_internal` の UTF-8 境界バグ (issue 0020) は
  client/server 双方に同じ実装が並んでいる構造に起因する。重複を残すと類似の
  「片側だけ修正される」バグを今後も繰り返す。
- 下流 issue の前提: 今後の改善（タイマー精度向上、ハートビート戦略の見直し等）は
  共通化された状態機械を前提に設計したほうが影響範囲を限定できる。

## 現状

### 重複箇所一覧

以下のメソッド/構造体/定数が client/server で実質同一の実装を持つ。
行番号は 0020 適用前の `develop` を基準にしているため、0020 適用後は
`close_internal` 周辺で数行ずれる可能性がある。実装時は最新の `develop` を基準に
再確認すること。

| メソッド/構造体                    | client 行 | server 行 | policy 引数 |
| ---------------------------------- | --------- | --------- | ----------- |
| `FragmentBuffer` (struct + impl)   | 247-294   | 122-169   | -           |
| `process_frames`                   | 847-866   | 818-837   | 必要        |
| `handle_decoded_frame`             | 868-879   | 839-845   | 必要        |
| `handle_frame`                     | 881-934   | 847-900   | 必要        |
| `handle_data_frame`                | 936-962   | 902-928   | 必要        |
| `handle_continuation`              | 964-990   | 930-956   | 必要        |
| `decompress_if_needed`             | 992-1013  | 958-979   | 必要        |
| `emit_message`                     | 1015-1044 | 981-1005  | 必要        |
| `handle_close`                     | 1046-1130 | 1007-1091 | 必要        |
| `handle_ping`                      | 1132-1145 | 1093-1106 | 必要        |
| `handle_pong`                      | 1147-1160 | 1108-1121 | 不要        |
| `close_internal`                   | 634-661   | 684-709   | 必要        |
| `send_data_frame`                  | 476-483   | 596-604   | 必要        |
| `compress_if_enabled`              | 486-499   | 607-620   | 不要        |
| `set_state`                        | 607-613   | 760-766   | 不要        |
| `check_connected`                  | 615-620   | 768-773   | 不要        |
| `handle_timer`                     | 560-593   | 712-746   | 必要        |
| `poll_event` / `poll_output`       | 596-603   | 749-756   | 不要        |
| `DEFAULT_MAX_FRAME_SIZE`           | 131       | 25        | -           |
| `DEFAULT_MAX_MESSAGE_SIZE`         | 134       | 28        | -           |
| `DEFAULT_MAX_DECOMPRESSED_SIZE`    | 137       | 31        | -           |

`decompress_if_needed` は内部で `self.close_internal(...)` を呼ぶ
（client L1002-1005, server L968-971）ため policy が必要。

### Connection 側に残留するメソッド

以下は public API 境界または各 Connection 固有のハンドシェイク制御のため
`SharedConnectionState` には移動しない。

| メソッド                                       | 残留理由                                                     |
| ---------------------------------------------- | ------------------------------------------------------------ |
| `new`                                          | options 型が異なる                                           |
| `connect` (client) / `accept_handshake` 系     | ハンドシェイクは非対称で共通化対象外                         |
| `process_handshake`                            | バリデーター型が client/server で異なる                      |
| `complete_handshake` (client)                  | client 固有のレスポンス検証                                  |
| `feed_recv_buf`                                | 状態に応じて handshake / frames へ振り分ける薄いラッパー     |
| `send_text` / `send_binary`                    | `SharedConnectionState::send_data_frame` への薄い委譲        |
| `send_ping`                                    | `check_connected` 検証 + `awaiting_pong` 更新の薄いラッパー  |
| `close`                                        | 送信禁止 close code の検証を行う public API                  |

### Connection 側に残留するフィールド

| フィールド               | client                       | server                          |
| ------------------------ | ---------------------------- | ------------------------------- |
| `options`                | `ClientConnectionOptions`    | `ServerConnectionOptions`       |
| `handshake_validator`    | `Option<HandshakeValidator>` | `HandshakeRequestValidator`     |
| `nonce`                  | `[u8; 16]`                   | なし                            |
| `pending_request`        | なし                         | `Option<ServerHandshakeRequest>`|
| `pending_frame_data`     | なし                         | `Vec<u8>`                       |
| `negotiated_protocol`    | `Option<String>`             | `Option<String>`                |
| `negotiated_extensions`  | `Vec<String>`                | `Vec<String>`                   |

`nonce` はハンドシェイク完了後は参照されないが、`connect()` 内で `request.build(self.nonce)?`
と `HandshakeValidator::new(self.nonce)` に同じ値を 2 回渡す経路があるため Connection 側に
残す（ローカル変数化への整理は本 issue のスコープ外とする）。
`negotiated_protocol` / `negotiated_extensions` はハンドシェイク完了時にのみ書き込まれ、
以降は `protocol()` / `extensions()` 経由の read-only 公開のため Shared 化しない。

### client/server で挙動が異なるメソッド

| メソッド     | 差異                                                                                                          | リファクタリング後                                  |
| ------------ | ------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| `send_frame` | client: masking + `Result<(), Error>` / server: masked なし + `()`                                            | `FramePolicy::encode_and_send` に統合して削除する   |

`send_frame` 削除後、呼び出し側 (`close`, `send_ping`, `send_data_frame`, `handle_ping`,
`handle_close`, `close_internal`) は `policy.encode_and_send(&frame, &mut self.shared);`
に置換される（戻り値 `()` のため `?` 不要）。
`Frame::ping(...)?` や `Frame::close(...)?` といったコンストラクタ側の `?` は **残る**。

### 未使用の `now: Timestamp` パラメータ

以下の 7 メソッドで client のみが `now: Timestamp` を受け取るがすべて未使用。共通化に
合わせて削除する。

| メソッド                | client 行 | server 行 |
| ----------------------- | --------- | --------- |
| `process_frames`        | 847       | 818       |
| `handle_decoded_frame`  | 868       | 839       |
| `handle_frame`          | 881       | 847       |
| `handle_data_frame`     | 936       | 902       |
| `handle_continuation`   | 964       | 930       |
| `handle_close`          | 1046      | 1007      |
| `emit_message`          | 1015      | 981       |

`complete_handshake(response, _now: Timestamp)` の `_now` および client `feed_recv_buf(buf, now)`
の `now` は public API なので削除しない（server `feed_recv_buf(buf)` との非対称は許容）。

## 設計方針

### 採用方針

共通フィールドを束ねた内部構造体 `SharedConnectionState` を抽出し、その `impl`
ブロックに共通メソッドを定義する。マスキング有無やフレームエンコードといった
client/server で挙動が分かれる部分のみ `FramePolicy` トレイトで抽象化し、
各 Connection は独立した `*FramePolicy` 構造体をフィールドとして保持する。

`SharedConnectionState` は型パラメータを取らない non-generic な構造体とする。
client は `ClientFramePolicy<R: RandomSource>` 経由で `R` の単相化を policy 側にだけ
押し込めるため、`SharedConnectionState` のフィールド群 (10 以上) を `R` ごとに
複製する必要がない。

### 設計判断の根拠

1. **トレイトのデフォルト実装ではなく構造体を選ぶ理由**:
   トレイト + デフォルト実装では実装側 struct の private field に直接アクセスできない。
   10 を超える共通フィールドに対して getter/setter を全部生やすのは割に合わない。
2. **`FramePolicy` を独立構造体に分ける理由**:
   `SharedConnectionState` のメソッドが `&mut self.shared` と `&mut policy` を同時に
   借りる必要があるため、Connection 構造体の独立フィールドに切り出すことで Rust の
   disjoint field borrow を成立させる。

### 借用に関するコーディング規約

- `SharedConnectionState` 内で `policy.encode_and_send(&frame, self)` を呼ぶ際は、
  `self` を引数として渡すので、呼び出しの「直前まで」に必要な `self.xxx` の書き込みを
  完了させること（呼び出し後に続けて `self.xxx` を操作するのは問題ない）。
- `SharedConnectionState` のメソッド内で別の Shared メソッドに `policy` を渡し直す場合
  （例: `process_frames` → `handle_decoded_frame`）、Rust の自動 reborrow が効くため
  `self.handle_decoded_frame(decoded, policy)?;` のように単に `policy` を渡せばよい。
  明示的な `&mut *policy` は不要。
- Connection 側で `self.shared.<method>(..., &mut self.policy)` を呼ぶときは、
  `&mut self.shared` と `&mut self.policy` の同時借用が disjoint field borrow として
  成立する。残留フィールド (`self.options.X`, `self.negotiated_protocol` 等) を
  同一式内で読む場合も、`self.shared` / 残留フィールドはそれぞれ異なるフィールドなので
  disjoint field borrow が成立し問題ない（例:
  `self.shared.event_queue.push_back(ConnectionEvent::Connected { protocol: self.negotiated_protocol.clone(), ... })` は通る）。
- `self.pending_frame_data` 等の他の `&mut` フィールドはローカル変数に
  `std::mem::take` で取り出してから渡せば衝突しない（既存 `accept_handshake` L534-537
  の書き方をそのまま踏襲する）。

## 完了条件

1. `cargo check --workspace` がエラーなしで通る
2. `cargo clippy --workspace --all-targets -- -D warnings` が警告なしで通る
3. `cargo test --workspace` が全件パスする
4. `cargo doc --workspace --no-deps` がエラーなしで通る
5. リファクタリング後の `cargo +nightly rustdoc --lib -- --output-format json` の
   `paths` がリファクタリング前と差分なしであることを確認する
6. 新規単体テスト（「テスト戦略」参照）が追加され全件パスする
7. `src/websocket_client_connection.rs` から `FragmentBuffer` 定義および
   `ConnectionState` / `TimerId` / `ConnectionEvent` / `ConnectionOutput` 定義が
   削除されている
8. `src/websocket_server_connection.rs` から `FragmentBuffer` 定義が削除されている
9. `src/websocket_frame.rs:107` の `encode_unmasked` から `#[allow(dead_code)]` が
   除去されている
10. `CHANGES.md` の `## develop` > `### misc` に変更履歴エントリが追加されている

## 解決方法

### Step 1: 共通型を独立モジュールに移動する

`src/connection_types.rs` を新規作成し、`ConnectionState`, `TimerId`,
`ConnectionEvent`, `ConnectionOutput` の 4 型の定義を
`src/websocket_client_connection.rs` から移動する。`RandomSource` トレイトは client 固有
なので `websocket_client_connection.rs` に残す。

`src/lib.rs` の置換: 既存の

```rust
pub use websocket_client_connection::{
    ClientConnectionOptions, ConnectionEvent, ConnectionOutput, ConnectionState, RandomSource,
    TimerId, WebSocketClientConnection,
};
```

を以下に差し替える。

```rust
mod connection_shared; // 既存 mod 群と並べて追加
mod connection_types;

pub use connection_types::{ConnectionEvent, ConnectionOutput, ConnectionState, TimerId};
pub use websocket_client_connection::{
    ClientConnectionOptions, RandomSource, WebSocketClientConnection,
};
```

server 側 (`src/websocket_server_connection.rs:18`) の
`use crate::{ConnectionEvent, ConnectionOutput, ConnectionState, TimerId};` は
`lib.rs` の `pub use` 経由でそのまま動くので変更は任意。スタイル統一として
`use crate::connection_types::{ConnectionEvent, ConnectionOutput, ConnectionState, TimerId};`
に書き換える。

### Step 2: `SharedConnectionState` と `FragmentBuffer` を定義する

`src/connection_shared.rs` を新規作成する。

必要な `use`:

```rust
use std::collections::VecDeque;

use crate::connection_types::{ConnectionEvent, ConnectionOutput, ConnectionState, TimerId};
use crate::deflate::PerMessageDeflate;
use crate::error::Error;
use crate::websocket_close::{CloseCode, truncate_reason};
use crate::websocket_frame::{DecodedFrame, Frame, FrameDecoder};
use crate::websocket_opcode::Opcode;
```

定数（両 Connection の `pub const` だが `lib.rs` から `pub use` されておらず外部 API
ではない。本 issue で `connection_shared.rs` に一元化し、各 Connection モジュール側で
`pub use crate::connection_shared::DEFAULT_MAX_*;` を追加してモジュールパスを保つ）:

```rust
pub const DEFAULT_MAX_FRAME_SIZE: usize = 64 * 1024 * 1024;
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;
pub const DEFAULT_MAX_DECOMPRESSED_SIZE: usize = 16 * 1024 * 1024;
```

`FragmentBuffer` は `pub(crate)` で定義し、メソッドもすべて `pub(crate) fn` にする
（`SharedConnectionState` 外のモジュールから直接触らないが、`SharedConnectionState`
は同じファイル内なのでデフォルト private でも動く。スタイル一貫性のため
`pub(crate)` で統一する）:

```rust
#[derive(Debug, Default)]
pub(crate) struct FragmentBuffer {
    opcode: Option<Opcode>,
    payload: Vec<u8>,
    compressed: bool,
}

impl FragmentBuffer {
    pub(crate) fn new() -> Self { Self::default() }
    pub(crate) fn is_empty(&self) -> bool { self.opcode.is_none() }
    pub(crate) fn len(&self) -> usize { self.payload.len() }
    pub(crate) fn start(&mut self, opcode: Opcode, payload: Vec<u8>, compressed: bool) {
        self.opcode = Some(opcode);
        self.payload = payload;
        self.compressed = compressed;
    }
    pub(crate) fn append(&mut self, payload: &[u8]) {
        self.payload.extend_from_slice(payload);
    }
    pub(crate) fn take(&mut self) -> (Opcode, Vec<u8>, bool) {
        let opcode = self.opcode.take().unwrap_or(Opcode::Binary);
        let payload = std::mem::take(&mut self.payload);
        let compressed = self.compressed;
        self.compressed = false;
        (opcode, payload, compressed)
    }
    pub(crate) fn clear(&mut self) {
        self.opcode = None;
        self.payload.clear();
        self.compressed = false;
    }
}
```

`SharedConnectionState` の定義:

```rust
pub(crate) struct SharedConnectionState {
    pub(crate) state: ConnectionState,
    pub(crate) close_sent: bool,
    pub(crate) close_received: bool,
    pub(crate) awaiting_pong: bool,
    pub(crate) failed: bool,
    pub(crate) event_queue: VecDeque<ConnectionEvent>,
    pub(crate) output_queue: VecDeque<ConnectionOutput>,
    pub(crate) frame_decoder: FrameDecoder,
    pub(crate) fragment_buffer: FragmentBuffer,
    pub(crate) deflate: Option<PerMessageDeflate>,
    pub(crate) max_frame_size: usize,
    pub(crate) max_message_size: usize,
    pub(crate) max_decompressed_size: usize,
    pub(crate) ping_interval_millis: u64,
    pub(crate) pong_timeout_millis: u64,
    pub(crate) close_timeout_millis: u64,
}

impl SharedConnectionState {
    pub(crate) fn new(
        max_frame_size: usize,
        max_message_size: usize,
        max_decompressed_size: usize,
        ping_interval_millis: u64,
        pong_timeout_millis: u64,
        close_timeout_millis: u64,
    ) -> Self {
        Self {
            state: ConnectionState::Disconnected,
            close_sent: false,
            close_received: false,
            awaiting_pong: false,
            failed: false,
            event_queue: VecDeque::new(),
            output_queue: VecDeque::new(),
            frame_decoder: FrameDecoder::new(),
            fragment_buffer: FragmentBuffer::new(),
            deflate: None,
            max_frame_size,
            max_message_size,
            max_decompressed_size,
            ping_interval_millis,
            pong_timeout_millis,
            close_timeout_millis,
        }
    }
}
```

### Step 3: `FramePolicy` トレイトとポリシー構造体を定義する

`src/connection_shared.rs` に追加する:

```rust
pub(crate) trait FramePolicy {
    /// フレームのマスク方向を検証する。
    /// `masked` は `DecodedFrame.masked` から取得する。
    /// `Frame` 自体には `masked` フィールドがないため、呼び出し元で分離して渡す。
    fn verify_frame_masking(&self, masked: bool) -> Result<(), Error>;

    /// フレームをエンコードして送信キューに追加する。
    fn encode_and_send(&mut self, frame: &Frame, shared: &mut SharedConnectionState);
}

pub(crate) struct ClientFramePolicy<R: RandomSource> {
    pub(crate) random: R,
}

impl<R: RandomSource> ClientFramePolicy<R> {
    pub(crate) fn new(random: R) -> Self {
        Self { random }
    }

    /// ハンドシェイク用の nonce を生成する (`connect()` から利用)。
    pub(crate) fn nonce(&mut self) -> [u8; 16] {
        self.random.nonce()
    }
}

impl<R: RandomSource> FramePolicy for ClientFramePolicy<R> {
    fn verify_frame_masking(&self, masked: bool) -> Result<(), Error> {
        // RFC 6455 Section 5.1: サーバーからのフレームはマスクしてはならない
        // RFC 6455 Section 7.4.1: 違反時は 1002 (protocol error) を使用してよい
        if masked {
            return Err(Error::protocol_violation("masked server frame"));
        }
        Ok(())
    }

    fn encode_and_send(&mut self, frame: &Frame, shared: &mut SharedConnectionState) {
        let masking_key = self.random.masking_key();
        let encoded = frame.encode(masking_key);
        shared
            .output_queue
            .push_back(ConnectionOutput::SendData(encoded));
    }
}

pub(crate) struct ServerFramePolicy;

impl FramePolicy for ServerFramePolicy {
    fn verify_frame_masking(&self, masked: bool) -> Result<(), Error> {
        // RFC 6455 Section 5.1: クライアントからのフレームはマスクしなければならない
        // RFC 6455 Section 7.4.1: 違反時は 1002 (protocol error) を使用してよい
        if !masked {
            return Err(Error::protocol_violation("unmasked client frame"));
        }
        Ok(())
    }

    fn encode_and_send(&mut self, frame: &Frame, shared: &mut SharedConnectionState) {
        // RFC 6455 Section 5.1: サーバーは送信フレームをマスクしてはならない
        // RFC 6455 Section 5.2: MASK=0 のとき Masking-Key field は存在しない
        let encoded = frame.encode_unmasked();
        shared
            .output_queue
            .push_back(ConnectionOutput::SendData(encoded));
    }
}
```

`src/websocket_frame.rs:107` の `encode_unmasked` から `#[allow(dead_code)]` を除去する。
本 issue 完了後は `ServerFramePolicy::encode_and_send` 経由で使用されるため不要となる。

### Step 4: `SharedConnectionState` に共通メソッドを実装する

「現状」の重複箇所一覧で `SharedConnectionState` 移動対象としたメソッド群を
そのまま移動する。policy 引数欄が「必要」のものは引数末尾に `policy: &mut impl FramePolicy`
を追加する。`now: Timestamp` パラメータは「現状」の表に従って削除する。

メソッド本体内の置換規則:

- 旧 `self.<shared 移動フィールド>` → `self.<shared 移動フィールド>` (Shared 内部なので
  `self` のまま; フィールド名は変わらない)
- 旧 `self.options.<x>` （Shared にコピーしたオプション値）→ `self.<x>`
  （例: `self.options.max_frame_size` → `self.max_frame_size`）
- 旧 `self.send_frame(frame)[?];` → `policy.encode_and_send(&frame, self);`
- 旧 `self.close(code, reason)?;`（`emit_message` と `handle_timer::PongTimeout` の 2 箇所）
  → `self.close_internal(code, reason, policy);`
- 旧 `self.close_internal(...)[?];` → `self.close_internal(..., policy);`

各メソッドの最終シグネチャ:

```text
fn set_state(&mut self, new_state: ConnectionState)
fn check_connected(&self) -> Result<(), Error>
fn close_internal(&mut self, code: CloseCode, reason: &str, policy: &mut impl FramePolicy)
fn send_data_frame(&mut self, opcode: Opcode, payload: Vec<u8>, policy: &mut impl FramePolicy) -> Result<(), Error>
fn compress_if_enabled(&mut self, payload: Vec<u8>) -> Result<(Vec<u8>, bool), Error>
fn decompress_if_needed(&mut self, payload: Vec<u8>, compressed: bool, policy: &mut impl FramePolicy) -> Result<Vec<u8>, Error>
fn process_frames(&mut self, buf: &[u8], policy: &mut impl FramePolicy) -> Result<(), Error>
fn handle_decoded_frame(&mut self, decoded: DecodedFrame, policy: &mut impl FramePolicy) -> Result<(), Error>
fn handle_frame(&mut self, frame: Frame, policy: &mut impl FramePolicy) -> Result<(), Error>
fn handle_data_frame(&mut self, frame: Frame, policy: &mut impl FramePolicy) -> Result<(), Error>
fn handle_continuation(&mut self, frame: Frame, policy: &mut impl FramePolicy) -> Result<(), Error>
fn emit_message(&mut self, opcode: Opcode, payload: Vec<u8>, policy: &mut impl FramePolicy) -> Result<(), Error>
fn handle_close(&mut self, frame: Frame, policy: &mut impl FramePolicy) -> Result<(), Error>
fn handle_ping(&mut self, frame: Frame, policy: &mut impl FramePolicy) -> Result<(), Error>
fn handle_pong(&mut self, frame: Frame) -> Result<(), Error>
fn handle_timer(&mut self, timer_id: TimerId, policy: &mut impl FramePolicy) -> Result<(), Error>
fn poll_event(&mut self) -> Option<ConnectionEvent>
fn poll_output(&mut self) -> Option<ConnectionOutput>
```

### Step 4-1: `close_internal` の最終形

0020 で `is_char_boundary` 切り詰めが `truncate_reason(reason, max_bytes)` 関数として
`src/websocket_close.rs` に切り出される前提（0020 のテスト戦略セクション）。
**ただし 0020 の「変更対象ファイルと影響範囲」には `close_internal` 本体を
`truncate_reason` 呼び出しに書き換える指示がない**。
本 issue 実装時には以下の手順で対応する:

1. 0020 完了時点で `close_internal` が `is_char_boundary` のインライン展開のままなら、
   本 issue の Step 4-1 着手前に `src/websocket_close.rs` の `truncate_reason` を
   呼び出す形に書き換える（client/server の `close_internal` 両方）
2. 0020 完了時点で既に書き換え済みなら、そのまま `SharedConnectionState::close_internal`
   に移動する

いずれの場合も最終形は以下:

```rust
fn close_internal(
    &mut self,
    code: CloseCode,
    reason: &str,
    policy: &mut impl FramePolicy,
) {
    if self.state == ConnectionState::Disconnected || self.state == ConnectionState::Closed {
        return;
    }

    if !self.close_sent {
        // truncate_reason で reason は 123 バイト以下に切り詰め済みのため、
        // 現在の Frame::close では Err は発生しないが、Frame::close の将来的な
        // エラー条件追加 (例: 予約 close code チェック) への堅牢性のため
        // unwrap_or_else フォールバックを維持する。
        let truncated = truncate_reason(reason, 123);
        let frame = Frame::close(Some(code.as_u16()), truncated)
            .unwrap_or_else(|_| Frame::close(Some(code.as_u16()), "").unwrap());
        policy.encode_and_send(&frame, self);
        self.close_sent = true;

        self.output_queue.push_back(ConnectionOutput::SetTimer {
            id: TimerId::CloseTimeout,
            duration_millis: self.close_timeout_millis,
        });

        self.set_state(ConnectionState::Closing);
    }
}
```

### Step 4-2: `handle_timer::TimerId::Ping` のインライン展開

`send_ping` は Connection 側に残るため `SharedConnectionState::handle_timer` から
直接呼べない。Ping 送信ロジックをインライン展開する。**`PongTimeout` タイマーの
セットも必ず含める**（既存の `send_ping` (client L513-516, server L634-637) と同じ動作を
保つ必要がある）。外側で `state == Connected` を確認済みのため、旧 `send_ping` 内の
`check_connected` 検証は省略しても等価。

```rust
TimerId::Ping => {
    if self.state == ConnectionState::Connected && !self.awaiting_pong {
        // RFC 6455 Section 5.5: 全コントロールフレームは 125 バイト以下。
        // 空ペイロード (0 バイト) は常に許容され Frame::ping は Err を返さないため
        // unwrap で安全。
        let frame = Frame::ping(Vec::new()).unwrap();
        policy.encode_and_send(&frame, self);
        self.awaiting_pong = true;
        self.output_queue.push_back(ConnectionOutput::SetTimer {
            id: TimerId::PongTimeout,
            duration_millis: self.pong_timeout_millis,
        });
    }
    if self.state == ConnectionState::Connected && self.ping_interval_millis > 0 {
        self.output_queue.push_back(ConnectionOutput::SetTimer {
            id: TimerId::Ping,
            duration_millis: self.ping_interval_millis,
        });
    }
}
```

`TimerId::PongTimeout` 分岐の `self.close(...)?;` は Step 4 の置換規則 (4 番目) に従い
`self.close_internal(..., policy);` に置換する。`awaiting_pong` フラグは PongTimeout
発火後もリセットしないが、`close_internal` が `state` を `Closing` に遷移させるため、
次の `Ping` タイマー発火時には `state == Connected` 条件で弾かれ Ping は送られない。

### Step 5: 各 Connection 構造体への組み込み

```rust
pub struct WebSocketClientConnection<R: RandomSource> {
    shared: SharedConnectionState,
    policy: ClientFramePolicy<R>,
    options: ClientConnectionOptions,
    nonce: [u8; 16],
    handshake_validator: Option<HandshakeValidator>,
    negotiated_protocol: Option<String>,
    negotiated_extensions: Vec<String>,
}

pub struct WebSocketServerConnection {
    shared: SharedConnectionState,
    policy: ServerFramePolicy,
    options: ServerConnectionOptions,
    handshake_validator: HandshakeRequestValidator,
    pending_request: Option<ServerHandshakeRequest>,
    pending_frame_data: Vec<u8>,
    negotiated_protocol: Option<String>,
    negotiated_extensions: Vec<String>,
}
```

`new()` 内では options のフィールドから `SharedConnectionState::new(...)` の引数を
渡す（client/server で同じ呼び出し形）:

```rust
shared: SharedConnectionState::new(
    options.max_frame_size,
    options.max_message_size,
    options.max_decompressed_size,
    options.ping_interval_millis,
    options.pong_timeout_millis,
    options.close_timeout_millis,
),
// client:
policy: ClientFramePolicy::new(random),
// server:
policy: ServerFramePolicy,
```

### Step 6: Connection 側に残るメソッドのフィールドアクセス書き換え

Connection 側に残る各メソッドに対して、機械的に以下の置換規則を適用する。残留
フィールド (`options`, `nonce`, `handshake_validator`, `pending_request`,
`pending_frame_data`, `negotiated_protocol`, `negotiated_extensions`) への参照は変更しない。

- `self.<shared 移動フィールド>` → `self.shared.<同名>`
- `self.<shared 移動メソッド>(...)` → `self.shared.<同名>(..., &mut self.policy)`
  （policy 引数が「必要」のメソッドのみ。「不要」は引数なしのまま）
- `self.send_frame(frame)[?];` → `self.policy.encode_and_send(&frame, &mut self.shared);`
- `self.random.nonce()` → `self.policy.nonce()`（client `connect` のみ）
- `Frame::xxx(...)?` のコンストラクタ側の `?` は残す
- public メソッド `state` / `protocol` / `extensions` / `poll_event` / `poll_output` /
  `handle_timer` は `self.shared.<同名>(...)` への単純委譲

対象メソッド（行範囲は実装時に再確認すること）:

- client: `connect` (398-440), `feed_recv_buf` (446-461), `send_text` (464-467),
  `send_binary` (470-473), `send_ping` (504-519), `close` (526-557),
  `process_handshake` (663-692), `complete_handshake` (694-845),
  公開メソッド `state` / `protocol` / `extensions` / `poll_event` / `poll_output` /
  `handle_timer`
- server: `feed_recv_buf` (264-281), `accept_handshake_auto` (294-315) は内部で
  `accept_handshake` を呼ぶだけなので書き換え不要, `accept_handshake` (318-541),
  `reject_handshake` (548-581), `process_handshake` (781-816), `send_text` (584-587),
  `send_binary` (590-593), `send_ping` (625-640), `close` (647-678),
  公開メソッド `state` / `protocol` / `extensions` / `poll_event` / `poll_output` /
  `handle_timer`

#### server `accept_handshake` の `process_frames` 呼び出しの書き換え例

3 種類のフィールド (Shared / 残留 / policy) を跨ぐため、置換規則の機械適用結果が
直感的でない箇所として例示する。実コード L534-537:

```rust
if !self.pending_frame_data.is_empty() {
    let pending = std::mem::take(&mut self.pending_frame_data);
    self.process_frames(&pending)?;
}
```

書き換え後:

```rust
if !self.pending_frame_data.is_empty() {
    let pending = std::mem::take(&mut self.pending_frame_data);
    self.shared.process_frames(&pending, &mut self.policy)?;
}
```

`pending` がローカル変数化されており `self.pending_frame_data` への借用が事前に
解放されているため、`&mut self.shared` と `&mut self.policy` の同時借用が成立する。

## 後方互換性

### 公開シンボル

`WebSocketClientConnection` / `WebSocketServerConnection` の `pub fn` および
`ConnectionState`, `TimerId`, `ConnectionEvent`, `ConnectionOutput`, `RandomSource`,
`ClientConnectionOptions`, `ServerConnectionOptions` の公開シンボルパス
(`shiguredo_websocket::<Name>`) は変更しない。`DEFAULT_MAX_*` は元々 `lib.rs` から
`pub use` されていないため外部からは到達不能。本 issue でも外部公開はしない。

特に client `feed_recv_buf(buf, now)` の `now: Timestamp` は内部フレーム処理から
未使用となるが public API として残す。

### 観測可能な挙動の変化

以下 2 点は意図的な内部挙動変更。テスト戦略で固定する。

1. **server `emit_message` の Close 呼び出し先**:
   server (現状 L994) で `self.close(CloseCode::INVALID_PAYLOAD, "invalid UTF-8")?;`
   と書かれていた箇所が `self.close_internal(...)` 経由に変わる。`close` が行っていた
   `is_sendable()` 検証は 1007 (INVALID_PAYLOAD) に対して常に通るため、外部から
   観測される Close フレーム送信動作 (RFC 6455 Section 8.1 の Fail the WebSocket
   Connection) は維持される。
2. **`handle_timer` の PongTimeout 戻り値**:
   `self.close(POLICY_VIOLATION, "pong timeout")?;` を `self.close_internal(...);` に
   置き換える。`close_internal` が値を返さないため `handle_timer` の戻り値は
   PongTimeout 発火時に必ず `Ok(())` となる（旧実装は理論上 `Err` を返す経路が
   存在したが、`Connected` / `Closing` 状態でしか発火しないため実用上は発生しない）。
   エラー検出は `ConnectionEvent::Error("pong timeout")` のイベントキュー経由に
   一本化される。

## テスト戦略

リファクタリングの主目的は外部挙動の維持なので、既存テスト（`tests/`,
`pbt/tests/`, `fuzz/`）が全件パスすることで回帰なしと判断する。
PBT は公開 API のみを使用しているため、`lib.rs` の `pub use` が Step 1 通りに
更新されればビルドに影響しない。

「後方互換性」で挙げた 2 つの意図的挙動変更については、PBT の strategy としては
状態遷移の自由度が低く（`emit_message` は `Connected` / `Closing` 状態でしか
到達しない）プロパティ化が薄くなるため、**単体テストとして固定する**。

`tests/test_connection_shared.rs` を新規作成し以下を追加する:

1. server で `String::from_utf8` 不正なテキスト 1 フレームを `feed_recv_buf` に流すと、
   送信キュー (`poll_output`) に Close フレーム (close code 1007) が積まれ、
   イベントキュー (`poll_event`) に `ConnectionEvent::Error` が積まれること
2. client/server 両方で `handle_timer(TimerId::PongTimeout)` を `awaiting_pong = true`
   状態で呼び出した際、`Ok(())` が返り、`ConnectionEvent::Error("pong timeout")` が
   イベントキューに積まれ、送信キューに Close フレーム (close code 1008) が積まれること

検証コマンド:

```bash
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```

## CHANGES.md

`## develop` > `### misc` に以下を追記する。担当者の `@実装者名` プレースホルダは
コミット前に実装者の GitHub ハンドルに差し替えること。

```text
### misc

- [UPDATE] クライアント/サーバー間のフレーム処理ロジックを SharedConnectionState に共通化する
  - @実装者名
```
