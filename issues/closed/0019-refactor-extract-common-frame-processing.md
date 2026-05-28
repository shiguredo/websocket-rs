# 0019: クライアント / サーバー間のフレーム処理ロジックの重複を解消する

- Priority: High
- Created: 2026-05-14
- Completed: 2026-05-26
- Polished: 2026-05-26
- Model: DeepSeek V4 Flash
- Branch: feature/refactor-extract-shared-connection-state

## 前提条件

本 issue は issue 0020 の完了を前提とする。CLAUDE.md の「番号が小さい issues から順番に対応すること」に対する例外であり、 0020 が先行する根拠は以下の通り:

- `close_internal` のバグ修正 (0020) は独立したバグ修正であり、リファクタリング (0019) とは責務が異なる
- 0020 を先に修正しないと、重複コードの両方にバグが残った状態で共通化することになり、共通化後のコードにもバグが混入する
- 0020 の修正量は小さく (関数 1 つの追加 + 戻り値型の統一)、0019 に統合すると issue の責務が曖昧になる

0020 から引き継ぐ前提条件:

1. `src/websocket_close.rs` に `truncate_reason` 関数が存在する
2. client 側 `close_internal` の戻り値型が `()` に統一済み
3. client 側の全呼び出し元で `close_internal(...)` の `?` が削除済み

本 issue のコード例・シグネチャはすべて 0020 完了後のコードベースを前提とする。

## 目的

`WebSocketClientConnection` と `WebSocketServerConnection` の間でフレーム処理ロジックが
コピーペーストで重複しており、 RFC 違反などのバグ修正時に両方の修正を強制される。
共通の状態 + 振る舞いを `SharedConnectionState` 構造体に切り出し、
マスキング有無などクライアント / サーバーで挙動が分かれる部分だけを `FramePolicy` トレイト
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

以下のメソッド / 構造体 / 定数が client/server で実質同一の実装を持つ。
実装時はメソッド名で `grep` して特定すること。

| メソッド / 構造体                  | policy 引数 | 備考                                                         |
| ---------------------------------- | ----------- | ------------------------------------------------------------ |
| `FragmentBuffer` (struct + impl)   | -           |                                                              |
| `process_frames`                   | 必要        |                                                              |
| `handle_decoded_frame`             | 必要        |                                                              |
| `handle_frame`                     | 必要        |                                                              |
| `handle_data_frame`               | 必要        |                                                              |
| `handle_continuation`              | 必要        |                                                              |
| `decompress_if_needed`             | 必要        | `deflate` が `None` のエラーパスで `close_internal` を呼ぶため |
| `emit_message`                     | 必要        |                                                              |
| `handle_close`                     | 必要        |                                                              |
| `handle_ping`                      | 必要        |                                                              |
| `handle_pong`                      | 不要        |                                                              |
| `close_internal`                   | 必要        |                                                              |
| `send_data_frame`                  | 必要        |                                                              |
| `compress_if_enabled`              | 不要        | `self` 参照のみで完結する                                    |
| `set_state`                        | 不要        |                                                              |
| `check_connected`                  | 不要        |                                                              |
| `handle_timer`                     | 必要        |                                                              |
| `poll_event` / `poll_output`       | 不要        |                                                              |
| `DEFAULT_MAX_FRAME_SIZE`           | -           |                                                              |
| `DEFAULT_MAX_MESSAGE_SIZE`         | -           |                                                              |
| `DEFAULT_MAX_DECOMPRESSED_SIZE`    | -           |                                                              |



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
| `send_ping`                                    | `check_connected` 検証 + `send_ping_internal` への委譲       |
| `close`                                        | 送信禁止 close code の検証を行う public API                  |
| `handshake_request` (server)                   | server 固有の public API                                     |
| `reject_handshake` (server)                    | server 固有のハンドシェイク拒否                              |
| `select_protocol` (server)                     | server 固有のプロトコルネゴシエーション                      |
| `select_deflate` (server)                      | server 固有の deflate ネゴシエーション                       |

### Connection 側に残留するフィールド

| フィールド / 定数               | client                       | server                           | 残留理由                                     |
| ------------------------------- | ---------------------------- | -------------------------------- | -------------------------------------------- |
| `options`                       | `ClientConnectionOptions`    | `ServerConnectionOptions`        | 型が異なる                                   |
| `handshake_validator`           | `Option<HandshakeValidator>` | `HandshakeRequestValidator`      | 型が異なる                                   |
| `nonce`                         | `[u8; 16]`                   | なし                             | `connect()` 内で 2 回参照                    |
| `pending_request`               | なし                         | `Option<ServerHandshakeRequest>` | server 固有                                  |
| `pending_frame_data`            | なし                         | `Vec<u8>`                        | server 固有                                  |
| `negotiated_protocol`           | `Option<String>`             | `Option<String>`                 | ハンドシェイク時のみ書き込み、以降 read-only |
| `negotiated_extensions`         | `Vec<String>`                | `Vec<String>`                    | 同上                                         |
| `MAX_PENDING_FRAME_DATA`        | なし                         | `const usize`                    | server 固有の定数                            |

### client/server で挙動が異なるメソッド

| メソッド     | 差異                                                                                                          | リファクタリング後                                  |
| ------------ | ------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| `send_frame` | client: masking + `Result<(), Error>` / server: masked なし + `()`                                            | `FramePolicy::encode_and_send` に統合して削除する   |

### 未使用の `now: Timestamp` パラメータ

以下の 7 メソッドで client のみが `now: Timestamp` を受け取るがすべて未使用。共通化に
合わせて削除する: `process_frames`, `handle_decoded_frame`, `handle_frame`,
`handle_data_frame`, `handle_continuation`, `handle_close`, `emit_message`。

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

トレイト + デフォルト実装ではなく構造体を選ぶ理由は、トレイトのデフォルト実装では
実装側 struct の private field に直接アクセスできず、10 を超える共通フィールドに
getter/setter を生やすコストが割に合わないため。`FramePolicy` を Connection 構造体の
独立フィールドに分けることで、`&mut self.shared` と `&mut self.policy` の
disjoint field borrow を成立させる。

`SharedConnectionState` のフィールドはすべて `pub(crate)` とする。Connection 側の
残留メソッドが `shared` の個別フィールド (`failed`, `close_sent`, `state` 等) に
直接アクセスする必要があり、全フィールドに getter/setter を追加するコストは割に合わない。

### 借用に関する注意

- `policy.encode_and_send(&frame, self)` の呼び出し前に `frame` をローカル変数に
  束縛すること。`self` を `&mut` で渡すため、`frame` の構築と送信を同一式で行えない。
- `self.pending_frame_data` 等の他の `&mut` フィールドはローカル変数に
  `std::mem::take` で取り出してから渡す（既存 `accept_handshake` の書き方を踏襲する）。

## 完了条件

1. `cargo check --workspace` がエラーなしで通る
2. `cargo clippy --workspace --all-targets -- -D warnings` が警告なしで通る
3. `cargo test --workspace` が全件パスする
4. `cargo doc --workspace --no-deps` がエラーなしで通る
5. `cargo doc --workspace --no-deps` の出力で `shiguredo_websocket::ConnectionState` 等の
   公開シンボルパスがリファクタリング前と同一であることを確認する
6. 新規単体テスト（「テスト戦略」参照）が追加され全件パスする
7. `src/websocket_client_connection.rs` から `FragmentBuffer` 定義、`ConnectionState` /
   `TimerId` / `ConnectionEvent` / `ConnectionOutput` 定義、`DEFAULT_MAX_FRAME_SIZE` /
   `DEFAULT_MAX_MESSAGE_SIZE` / `DEFAULT_MAX_DECOMPRESSED_SIZE` 定数が削除されている
8. `src/websocket_server_connection.rs` から `FragmentBuffer` 定義、
   `ConnectionState` / `TimerId` / `ConnectionEvent` / `ConnectionOutput` の
   `use crate::{...}` によるインポート、`DEFAULT_MAX_FRAME_SIZE` /
   `DEFAULT_MAX_MESSAGE_SIZE` / `DEFAULT_MAX_DECOMPRESSED_SIZE` 定数が削除されている
9. `src/websocket_frame.rs` の `encode_unmasked` から `#[allow(dead_code)]` が除去されている
10. `CHANGES.md` の `## develop` > `### misc` に変更履歴エントリが追加され、
    `@実装者名` プレースホルダが実装者の GitHub ハンドルに置換されている

## 解決方法

### Step 1: 共通型を独立モジュールに移動する

`src/websocket_connection_types.rs` を新規作成し、`ConnectionState`, `TimerId`,
`ConnectionEvent`, `ConnectionOutput`, `RandomSource` の 5 型の定義を
`src/websocket_client_connection.rs` から移動する。

`RandomSource` は client 固有のトレイトだが、`ClientFramePolicy<R: RandomSource>` が
`websocket_connection_shared.rs` に定義され `RandomSource` を型パラメータ境界に使う。
`RandomSource` を `websocket_client_connection.rs` に残すと
`websocket_connection_shared.rs` → `websocket_client_connection.rs` の依存が生じ、
逆方向の依存と循環するため `websocket_connection_types.rs` に移動する。
各ファイルへの型定義の配置後、削除元ファイルに `use crate::websocket_connection_types::{...};`
を追加してから元定義を削除する（削除と追加を同時に行わないと中間状態でコンパイルエラーになる）。

`src/lib.rs` の置換: 既存の

```rust
pub use websocket_client_connection::{
    ClientConnectionOptions, ConnectionEvent, ConnectionOutput, ConnectionState, RandomSource,
    TimerId, WebSocketClientConnection,
};
```

を以下に差し替える。

```rust
mod websocket_connection_shared; // 既存 mod 群と並べて追加
mod websocket_connection_types;

pub use websocket_connection_types::{ConnectionEvent, ConnectionOutput, ConnectionState, RandomSource, TimerId};
pub use websocket_client_connection::{
    ClientConnectionOptions, WebSocketClientConnection,
};
```

server 側 (`src/websocket_server_connection.rs`) の
`use crate::{ConnectionEvent, ConnectionOutput, ConnectionState, TimerId};` は
スタイル統一のため
`use crate::websocket_connection_types::{ConnectionEvent, ConnectionOutput, ConnectionState, TimerId};`
に書き換える。

### Step 2: `SharedConnectionState` と `FragmentBuffer` を定義する

`src/websocket_connection_shared.rs` を新規作成する。

必要な `use`:

```rust
use std::collections::VecDeque;

use crate::websocket_connection_types::{ConnectionEvent, ConnectionOutput, ConnectionState, RandomSource, TimerId};
use crate::deflate::PerMessageDeflate;
use crate::error::Error;
use crate::websocket_close::{CloseCode, truncate_reason};
use crate::websocket_frame::{DecodedFrame, Frame, FrameDecoder};
use crate::websocket_opcode::Opcode;
```

定数（両 Connection の `pub const` だが `lib.rs` から `pub use` されておらず外部 API
ではない。本 issue で `websocket_connection_shared.rs` に一元化し、各 Connection モジュール側で
`pub use crate::websocket_connection_shared::DEFAULT_MAX_*;` を追加してモジュールパスを保つ）:

```rust
pub const DEFAULT_MAX_FRAME_SIZE: usize = 64 * 1024 * 1024;
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;
pub const DEFAULT_MAX_DECOMPRESSED_SIZE: usize = 16 * 1024 * 1024;
```

`FragmentBuffer` は `pub(crate)` で定義し、メソッドもすべて `pub(crate) fn` にする:

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
        let opcode = self.opcode.take().expect("FragmentBuffer::take called on empty buffer");
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

`src/websocket_connection_shared.rs` に追加する:

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
        // RFC 6455 Section 5.1, Section 7.4.1: 違反時は 1002 (protocol error) を使用してよい
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
        // RFC 6455 Section 5.1, Section 7.4.1: 違反時は 1002 (protocol error) を使用してよい
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

`src/websocket_frame.rs` の `encode_unmasked` に付与されている `#[allow(dead_code)]` を除去する。
リファクタリング前の時点で `server::send_frame` から呼ばれており dead code ではなく、
リファクタリング後は `ServerFramePolicy::encode_and_send` から呼ばれるため不要。

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
fn send_ping_internal(&mut self, data: &[u8], policy: &mut impl FramePolicy) -> Result<(), Error>
fn poll_event(&mut self) -> Option<ConnectionEvent>
fn poll_output(&mut self) -> Option<ConnectionOutput>
```

`send_data_frame` の最終形。`compress_if_enabled` は `self` 参照のみで完結するため policy 不要。

```rust
fn send_data_frame(
    &mut self,
    opcode: Opcode,
    payload: Vec<u8>,
    policy: &mut impl FramePolicy,
) -> Result<(), Error> {
    let (payload, compressed) = self.compress_if_enabled(payload)?;
    let mut frame = Frame::new(opcode, payload);
    frame.rsv1 = compressed;
    policy.encode_and_send(&frame, self);
    Ok(())
}
```

### Step 4-0: `handle_decoded_frame` の最終形

`verify_frame_masking` は `&self` で純粋な検証のみを行い、Close フレーム送信は行わない。
RFC 6455 Section 5.1 の要求に従い、検証失敗時には `handle_decoded_frame` 側で明示的に
`close_internal` を呼ぶ。

```rust
fn handle_decoded_frame(
    &mut self,
    decoded: DecodedFrame,
    policy: &mut impl FramePolicy,
) -> Result<(), Error> {
    if let Err(e) = policy.verify_frame_masking(decoded.masked) {
        self.close_internal(CloseCode::PROTOCOL_ERROR, &e.to_string(), policy);
        return Err(e);
    }
    self.handle_frame(decoded.frame, policy)
}
```

`Error::to_string()` はエラーメッセージを返すため、`close_internal` の reason と
`verify_frame_masking` のエラーメッセージが一致する。

### Step 4-1: `close_internal` の最終形

issue 0020 で `truncate_reason` が追加済みであることを前提とする。最終形は以下:

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
        // truncate_reason 後は reason が常に 123 バイト以下だが、
        // Frame::close の将来的なエラー条件追加に備えて unwrap_or_else を維持する。
        let truncated = truncate_reason(reason, 123);
        let frame = Frame::close(Some(code.as_u16()), truncated)
            .unwrap_or_else(|_| Frame::close(Some(code.as_u16()), "").expect("empty reason close frame must always succeed"));
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

### Step 4-2: `send_ping_internal` の抽出

Ping 送信ロジックは `handle_timer::Ping` 分岐と Connection 側 `send_ping`（public API）の
2 箇所で重複する。`SharedConnectionState` に内部メソッドとして抽出し、両方から委譲する。

```rust
/// Ping フレームを送信し、awaiting_pong フラグと PongTimeout タイマーを設定する
fn send_ping_internal(
    &mut self,
    data: &[u8],
    policy: &mut impl FramePolicy,
) -> Result<(), Error> {
    let frame = Frame::ping(data.to_vec())?;
    policy.encode_and_send(&frame, self);
    self.awaiting_pong = true;
    self.output_queue.push_back(ConnectionOutput::SetTimer {
        id: TimerId::PongTimeout,
        duration_millis: self.pong_timeout_millis,
    });
    Ok(())
}
```

`handle_timer::Ping` 分岐は以下に置き換わる:

```rust
TimerId::Ping => {
    if self.state == ConnectionState::Connected && !self.awaiting_pong {
        self.send_ping_internal(&[], policy)?;
    }
    if self.state == ConnectionState::Connected && self.ping_interval_millis > 0 {
        self.output_queue.push_back(ConnectionOutput::SetTimer {
            id: TimerId::Ping,
            duration_millis: self.ping_interval_millis,
        });
    }
}
```

`TimerId::PongTimeout` 分岐の最終形:

```rust
TimerId::PongTimeout => {
    if self.awaiting_pong {
        self.event_queue
            .push_back(ConnectionEvent::Error("pong timeout".to_string()));
        self.close_internal(CloseCode::POLICY_VIOLATION, "pong timeout", policy);
    }
}
```

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

対象メソッド:

- client: `connect`, `feed_recv_buf`, `send_text`, `send_binary`, `send_ping`,
  `close`, `process_handshake`, `complete_handshake`,
  公開メソッド `state` / `protocol` / `extensions` / `poll_event` / `poll_output` /
  `handle_timer`
- server: `feed_recv_buf`, `accept_handshake_auto` (内部で `accept_handshake` を呼ぶ
  だけなので書き換え不要), `accept_handshake`, `reject_handshake`,
  `process_handshake`, `send_text`, `send_binary`, `send_ping`, `close`,
  公開メソッド `state` / `protocol` / `extensions` / `poll_event` / `poll_output` /
  `handle_timer`

#### client/server `feed_recv_buf` の書き換え例

client `feed_recv_buf` の `process_frames` 呼び出しは `now` 引数が削除される。
`failed` フラグや `handshake_validator` を含むその他のロジックは残留:

```rust
// client feed_recv_buf 書き換え後
pub fn feed_recv_buf(&mut self, buf: &[u8], now: Timestamp) -> Result<(), Error> {
    if self.shared.failed {
        return Err(Error::invalid_state("connection has failed"));
    }
    let result = match self.shared.state {
        ConnectionState::Connecting => self.process_handshake(buf, now),
        ConnectionState::Connected | ConnectionState::Closing => {
            self.shared.process_frames(buf, &mut self.policy)
        }
        ConnectionState::Disconnected | ConnectionState::Closed => {
            return Err(Error::invalid_state("connection is closed"));
        }
    };
    if result.is_err() {
        self.shared.failed = true;
    }
    result
}
```

サーバー側は `now` 引数がない。`pending_frame_data` / `handshake_validator` は Connection 側に残留:

```rust
// server feed_recv_buf 書き換え後
pub fn feed_recv_buf(&mut self, buf: &[u8]) -> Result<(), Error> {
    if self.shared.failed {
        return Err(Error::invalid_state("connection has failed"));
    }
    let result = match self.shared.state {
        ConnectionState::Disconnected | ConnectionState::Connecting => {
            self.process_handshake(buf)
        }
        ConnectionState::Connected | ConnectionState::Closing => {
            self.shared.process_frames(buf, &mut self.policy)
        }
        ConnectionState::Closed => {
            return Err(Error::invalid_state("connection is closed"));
        }
    };
    if result.is_err() {
        self.shared.failed = true;
    }
    result
}
```

#### client `send_ping` と `close` の書き換え例

`send_ping` と `close` は Connection 側に残留するが、`options.pong_timeout_millis` /
`options.close_timeout_millis` は Shared 側の同名フィールドに置き換える
（値は `SharedConnectionState::new()` でコピー済みのため同一）:

```rust
// client send_ping 書き換え後
pub fn send_ping(&mut self, data: &[u8]) -> Result<(), Error> {
    self.shared.check_connected()?;
    self.shared.send_ping_internal(data, &mut self.policy)
}

// client close 書き換え後
// 入力バリデーションのみ行い、Close フレーム送信は close_internal に委譲する。
// close_internal 内の truncate_reason は reason が 123 バイト以下なら no-op。
pub fn close(&mut self, code: CloseCode, reason: &str) -> Result<(), Error> {
    if !matches!(
        self.shared.state,
        ConnectionState::Connected | ConnectionState::Closing
    ) {
        return Err(Error::invalid_state("connection is not established"));
    }
    if !code.is_sendable() {
        let code_val = code.as_u16();
        return Err(Error::invalid_input(format!(
            "close code {} is not sendable",
            code_val
        )));
    }
    // reason が 123 バイト超の場合は Frame::close がエラーを返す。
    // close_internal は truncate_reason で切り詰めるが、public API では呼び出し元に
    // エラーとして通知する。
    if reason.len() > 123 {
        return Err(Error::invalid_input(format!(
            "close reason exceeds 123 bytes: {} bytes",
            reason.len()
        )));
    }
    self.shared.close_internal(code, reason, &mut self.policy);
    Ok(())
}
```

#### 実装上の注意: Step 4 〜 Step 6 はアトミックに行う

Step 4（Shared メソッド）、Step 5（構造体フィールド組み込み）、Step 6（Connection 側
メソッドの書き換え）は、いずれか 1 つだけ完了してもコンパイルが通らない。
3 ステップを atomic な 1 変更として扱い、すべて完了してから `cargo check` で検証すること。

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

以下 3 点は意図的な内部挙動変更。テスト戦略で固定する。

1. **server `emit_message` の Close 呼び出し先**:
   `self.close(...)` が `self.close_internal(...)` に変わる。`close()` は
   `is_sendable()` チェックと状態チェック (`Connected | Closing`) を行うが、
   `close_internal()` は `Disconnected | Closed` のガードのみ行う。
   `emit_message` は `Connected | Closing` 状態でのみ到達するため状態チェックは自明に通過する。
   渡される `CloseCode::INVALID_PAYLOAD` (1007) は `is_sendable() == true` のため
   送信禁止チェックも実害なし。Close フレーム送信動作は維持される。
2. **client/server `handle_timer::PongTimeout` の Close 呼び出し先**:
   client 側: `self.close(...)` が `self.close_internal(...)` に変わる。
   server 側: `self.close(...)` が `self.close_internal(...)` に変わる。
   渡される `CloseCode::POLICY_VIOLATION` (1008) は `is_sendable() == true` のため実害なし。
   `close_internal` が `()` を返すため戻り値は常に `Ok(())` となる。
   エラー検出は `ConnectionEvent::Error("pong timeout")` のイベントキュー経由に一本化される。
3. **client/server `handle_timer::Ping` の send_ping 呼び出し先**:
   public `send_ping` から内部の `send_ping_internal` に変わる。`send_ping` 内の
   `check_connected()` は外側の `state == Connected` ガードにより自明に通過するため
   実害なし。この最終形は client/server 両方の `handle_timer::Ping` 分岐を置き換える。

## テスト戦略

リファクタリングの主目的は外部挙動の維持なので、既存テスト（`tests/`,
`pbt/tests/`, `fuzz/`）が全件パスすることで回帰なしと判断する。
PBT は公開 API のみを使用しているため、`lib.rs` の `pub use` が Step 1 通りに
更新されればビルドに影響しない。

「後方互換性」で挙げた 3 つの意図的挙動変更のうち 1・2 については、PBT の strategy としては
状態遷移の自由度が低く（`emit_message` は `Connected` / `Closing` 状態でしか
到達しない）プロパティ化が薄くなるため、**単体テストとして固定する**。

`tests/` ディレクトリが存在しない場合は新規作成する。
`tests/test_websocket_connection_shared.rs` を新規作成し以下を追加する。
`SharedConnectionState` は `pub(crate)` のため、テストは `WebSocketClientConnection` /
`WebSocketServerConnection` の公開 API 経由でのみアクセス可能。
テストのセットアップでは必ずハンドシェイクを完了させてから `feed_recv_buf` に
データを流すこと。

1. server で不正 UTF-8 を含むテキスト 1 フレームを `feed_recv_buf` に流すと、
   送信キュー (`poll_output`) に Close フレーム (close code 1007) が積まれ、
   イベントキュー (`poll_event`) に `ConnectionEvent::Error` が積まれること。
   リファクタリング前は `emit_message` 内で `self.close()` を呼んでおり
   `is_sendable()` チェックが入るが、リファクタリング後は `self.close_internal()` を
   呼ぶためチェックがスキップされる。Close フレーム送信という最終挙動が同一であることを検証する
2. client/server 両方で `handle_timer(TimerId::PongTimeout)` を `awaiting_pong = true`
   状態で呼び出した際、`Ok(())` が返り、`ConnectionEvent::Error("pong timeout")` が
   イベントキューに積まれ、送信キューに Close フレーム (close code 1008) が積まれること
3. client で masked フレーム（サーバーからマスク付きフレーム）を `feed_recv_buf` に流すと、
   Close フレーム (close code 1002) が送信されエラーが返ること
   (`FramePolicy::verify_frame_masking` の検証)
4. server で unmasked フレーム（クライアントからマスクなしフレーム）を `feed_recv_buf` に
   流すと、Close フレーム (close code 1002) が送信されエラーが返ること

`send_ping_internal` は既存の Ping/Pong 関連テスト（PBT の `prop_client_connection.rs`,
`prop_server_connection.rs`）で間接的にカバーされる。

検証コマンド:

```bash
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```

## CHANGES.md

外部 API に変更のないリファクタリングだが、内部構造の改善として `[UPDATE]` を使用し
`### misc` セクションに記載する。担当者の `@実装者名` プレースホルダは
コミット前に実装者の GitHub ハンドルに差し替えること。

```text
- [UPDATE] クライアント / サーバー間のフレーム処理ロジックを SharedConnectionState に共通化する
  - @実装者名
```
