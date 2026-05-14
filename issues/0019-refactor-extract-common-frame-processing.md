# 0019: クライアント/サーバー間のフレーム処理ロジックの重複を解消する

Created: 2026-05-14
Model: deepseek-v4-flash

## 優先度

P1

## 概要

`WebSocketClientConnection` と `WebSocketServerConnection` の間でフレーム処理ロジックが
コピーペーストされており、400 行以上が完全に重複している。
バグ修正時に両方の修正が必要で、見落としが恒常的に発生する。

## 依存する issue

実装順序は以下の固定順序とする:

1. **issue 0020**: `close_internal` の UTF-8 境界切り詰め修正 + 戻り値型 `()` への統一
   (0020 で `close_internal` の戻り値型が `()` に変わるため、`?` → `;` 変更も 0020 で対応済み)
2. **本 issue (0019)**: 0020 修正済みのコードを対象に共通トレイトへの抽出を行う

本 issue では `close_internal` の戻り値型変更は行わず、0020 の成果をそのまま利用する。

## 重複箇所一覧

以下のメソッド/構造体がほぼ完全に同一:

| メソッド/構造体 | client 行 | server 行 |
|---|---|---|
| `FragmentBuffer` (struct + impl) | 248-294 | 123-169 |
| `handle_frame` | 881-934 | 847-900 |
| `handle_data_frame` | 936-962 | 902-928 |
| `handle_continuation` | 964-990 | 930-956 |
| `decompress_if_needed` | 993-1013 | 959-979 |
| `emit_message` | 1015-1044 | 981-1005 |
| `handle_close` | 1046-1130 | 1007-1091 |
| `handle_ping` | 1132-1145 | 1093-1106 |
| `handle_pong` | 1147-1160 | 1108-1121 |
| `handle_decoded_frame` | 868-879 | 839-845 |
| `close_internal` | 634-661 | 684-709 |
| `close` | 526-557 | 647-678 |
| `send_ping` | 504-519 | 625-640 |
| `send_data_frame` | 476-483 | 596-604 |
| `compress_if_enabled` | 486-499 | 607-620 |
| `set_state` | 607-613 | 760-766 |
| `check_connected` | 615-620 | 768-773 |
| `process_frames` | 847-866 | 818-837 |
| `handle_timer` | 560-593 | 712-746 |
| `poll_event` / `poll_output` | 596-603 | 749-756 |
| `DEFAULT_MAX_FRAME_SIZE` | 131 | 25 |
| `DEFAULT_MAX_MESSAGE_SIZE` | 134 | 28 |
| `DEFAULT_MAX_DECOMPRESSED_SIZE` | 137 | 31 |

## client/server で異なるメソッド

以下のメソッドは client/server でロジックが異なるため共通化しない。
ただし `send_frame` は `FramePolicy::encode_and_send` に置き換わるため、
リファクタリング後はデッドコードとなり **削除する**。

| メソッド | client | server | 差異 | リファクタリング後 |
|---|---|---|---|---|
| `send_frame` | 623-629 | 775-779 | マスキング有無 + 戻り値型 (client: `Result<(), Error>`, server: `()`) | `FramePolicy::encode_and_send` に置き換わるため削除 |

## 重複コード内に埋め込まれた差異

重複テーブルのメソッドは一見同一だが、以下の差異が内部に埋め込まれている。
共通化の際に吸収が必要:

### 1. `close_internal` の戻り値型

client: `fn close_internal(...) -> Result<(), Error>`
server: `fn close_internal(...)` (戻り値 `()`)

issue 0020 で client 側の戻り値型 `()` への統一 + 全呼び出し元の `?` → `;` 変更が対応されるため、
本 issue では考慮不要。

### 2. `emit_message` のクローズ呼び出し先

client (1033): `self.close_internal(CloseCode::INVALID_PAYLOAD, "invalid UTF-8")?;`
server (994): `self.close(CloseCode::INVALID_PAYLOAD, "invalid UTF-8")?;`

client は内部メソッド `close_internal` を、server は公開メソッド `close` を呼んでいる。
`close` は state チェック (`Connected`/`Closing`) を行うが、`close_internal` は行わない。
プロトコル違反時の挙動が異なるため統一が必要。

本 issue では `emit_message` が呼び出す先を `close_internal` に統一する。
server 側の `self.close(...)?;` は `self.close_internal(...);` に変更する。
（`close_internal` は server 側で元々 `()` を返すため、`?` 除去も 0019 の変更に含まれる。）

理由は以下の通り:

- `emit_message` は `SharedConnectionState` のメソッドとなるため、
  各 Connection の公開メソッド `close` を呼び出す手段がない（設計上の必然）
- プロトコル違反検出時は既に `Connected` または `Closing` 状態であるため、
  `close_internal` が追加の state チェックなしに動作しても安全

### 3. `now: Timestamp` パラメータ

以下の 7 メソッドで client のみが `now: Timestamp` を受け取るが、すべて未使用:

| メソッド | client 行 | server 行 |
|---|---|---|
| `process_frames` | 847 | 818 |
| `handle_decoded_frame` | 868 | 839 |
| `handle_frame` | 881 | 847 |
| `handle_data_frame` | 936 | 902 |
| `handle_continuation` | 964 | 930 |
| `handle_close` | 1046 | 1007 |
| `emit_message` | 1015 | 981 |

Sans-I/O パターンではハンドシェイクタイムアウトなどで `Timestamp` を利用する可能性があるが、
フレーム処理メソッドでは現状不要。本 issue では削除する（必要になったら追加する）。

なお削除後、`WebSocketClientConnection::feed_recv_buf` の `now: Timestamp` パラメータは
ハンドシェイク中の `process_handshake` にのみ使われ、フレーム処理時は未使用になる。
サーバーの `feed_recv_buf` は元々 `now` パラメータを持たないため、この非対称性は許容する。

### 4. `send_frame` の戻り値型の呼び出し側への浸透

client の `send_frame` が `Result<(), Error>` を返すため、以下の呼び出し側で
client/server のコードが `?` の有無で異なっている:

| メソッド | client | server |
|---|---|---|
| `send_ping` | `self.send_frame(frame)?;` | `self.send_frame(frame);` |
| `handle_ping` | `self.send_frame(pong)?;` | `self.send_frame(pong);` |
| `handle_close` | `self.send_frame(reply_frame)?;` | `self.send_frame(reply_frame);` |
| `send_data_frame` | `self.send_frame(frame)` (最終式として Result を伝播) | `self.send_frame(frame);` |

client の `send_frame` は常に `Ok(())` を返すため、`?` → `;` に変更して安全。
0020 で `close_internal` の戻り値型が `()` になり、`close_internal` 内部の
`self.send_frame(frame)?;` も `self.send_frame(frame);` に変更済みとなる。
本 issue では `encode_and_send` 経由でこの差異を吸収するため、呼び出し側の `?` は不要になる。

## 共通型の定義位置

以下の 4 型は現在 `websocket_client_connection.rs` で定義され、`lib.rs` 経由で
server が import している:

- `ConnectionState`
- `TimerId`
- `ConnectionEvent`
- `ConnectionOutput`

共通トレイトの切り出し時に、これらの型を独立モジュール
（例: `src/connection_types.rs`）に移動する。

## 修正方針

設計方針として、共通フィールドを束ねた内部構造体 `SharedConnectionState` を抽出し、
その impl ブロックに共通メソッドを定義するアプローチを採用する。
（トレイト + デフォルト実装では struct の private field にアクセスできないため不可）

Trait は client の `<R: RandomSource>` ジェネリクスに依存しないように
フレームエンコードとマスク検証のみを抽象化するために使用する。
`FramePolicy` トレイトは Connection 構造体自体ではなく独立したポリシー構造体に実装する
（借用チェッカー回避のため）。

### Step 1: 共通フィールドを束ねた内部構造体を定義する

`src/connection_shared.rs` を作成し、以下のフィールドを持つ
`SharedConnectionState` を定義する。可視性は `pub(crate)`:

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

    // オプション値（個別フィールドとして保持）
    pub(crate) max_frame_size: usize,
    pub(crate) max_message_size: usize,
    pub(crate) max_decompressed_size: usize,
    pub(crate) ping_interval_millis: u64,
    pub(crate) pong_timeout_millis: u64,
    pub(crate) close_timeout_millis: u64,
}
```

両 Connection はこの構造体をフィールドとして持つ。`random` は
`ClientFramePolicy`（Step 3 で定義）に移動する:

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

`negotiated_protocol`、`negotiated_extensions` はハンドシェイク時に設定され、
Connection 側から参照される。`SharedConnectionState` のフィールドではないため
`self.xxx` のまま変更不要。

`DEFAULT_MAX_*` 定数は `src/connection_shared.rs` に集約し、
`SharedConnectionState::new()` でデフォルト値として使用する:

```rust
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

各 Connection の `new()` 内では options からパラメータを渡す:

```rust
// client
SharedConnectionState::new(
    options.max_frame_size,
    options.max_message_size,
    options.max_decompressed_size,
    options.ping_interval_millis,
    options.pong_timeout_millis,
    options.close_timeout_millis,
)

// server (同様)
SharedConnectionState::new(
    options.max_frame_size,
    options.max_message_size,
    options.max_decompressed_size,
    options.ping_interval_millis,
    options.pong_timeout_millis,
    options.close_timeout_millis,
)
```

### Step 2: `SharedConnectionState` に共通メソッドを実装する

以下のメソッドを `impl SharedConnectionState` ブロックに移動する。
`now: Timestamp` パラメータは削除する（必要になったら追加する）。
これらのメソッドのうち、フレーム送信やマスク検証が必要なものは
`policy: &mut impl FramePolicy` を追加の引数として受け取る。

- `set_state`, `check_connected`
- `close_internal` — 戻り値型は 0020 で `()` に統一済み。policy が必要（Close フレーム送信）
- `send_data_frame` — policy が必要（データフレーム送信）
- `compress_if_enabled`
- `handle_frame` — policy が必要（close/ping/pong の送信）
- `handle_data_frame`, `handle_continuation` — policy が必要（close 送信）
- `decompress_if_needed`
- `emit_message` — policy が必要（close 送信）。呼び出し先は `close_internal` に統一
- `handle_close` — policy が必要（reply Close 送信）
- `handle_ping` — policy が必要（Pong 返信）
- `handle_pong`
- `handle_decoded_frame` — policy が必要（verify_frame_masking + handle_frame 呼び出し）
- `process_frames` — policy が必要（handle_decoded_frame 呼び出し）
- `handle_timer` — policy が必要（send_ping / close_internal 呼び出し）
- `poll_event`, `poll_output`

### Step 3: フレーム送信/マスク処理を抽象化するトレイトを定義する

トレイト名は `FramePolicy` とし、接続構造体自体ではなく独立したポリシー構造体に実装する。
接続構造体に直接実装すると `self.shared` (フィールド) と `self` (as `&impl FramePolicy`)
の同時借用が Rust の借用チェッカーで拒否されるため。

`src/connection_shared.rs` に以下を定義する:

```rust
pub(crate) trait FramePolicy {
    /// フレームのマスク方向を検証する
    ///
    /// `masked` は `DecodedFrame.masked` から取得する。
    /// `Frame` 構造体自体には `masked` フィールドは存在しないため、
    /// 呼び出し元で `DecodedFrame` から分離して渡す。
    fn verify_frame_masking(&self, masked: bool) -> Result<(), Error>;

    /// フレームをエンコードして送信キューに追加する
    fn encode_and_send(&mut self, frame: &Frame, shared: &mut SharedConnectionState);
}
```

client/server それぞれのポリシー構造体を定義する:

```rust
pub(crate) struct ClientFramePolicy<R: RandomSource> {
    pub(crate) random: R,
}

impl<R: RandomSource> FramePolicy for ClientFramePolicy<R> {
    fn verify_frame_masking(&self, masked: bool) -> Result<(), Error> {
        // RFC 6455 Section 5.1: サーバーからのフレームはマスクしてはならない
        if masked {
            return Err(Error::protocol_violation("masked server frame"));
        }
        Ok(())
    }
    fn encode_and_send(&mut self, frame: &Frame, shared: &mut SharedConnectionState) {
        let masking_key = self.random.masking_key();
        let encoded = frame.encode(masking_key);
        shared.output_queue.push_back(ConnectionOutput::SendData(encoded));
    }
}

pub(crate) struct ServerFramePolicy;

impl FramePolicy for ServerFramePolicy {
    fn verify_frame_masking(&self, masked: bool) -> Result<(), Error> {
        // RFC 6455 Section 5.1: クライアントからのフレームはマスク必須
        if !masked {
            return Err(Error::protocol_violation("unmasked client frame"));
        }
        Ok(())
    }
    fn encode_and_send(&mut self, frame: &Frame, shared: &mut SharedConnectionState) {
        // サーバーはマスクなし (RFC 6455 Section 5.1)
        let encoded = frame.encode_unmasked();
        shared.output_queue.push_back(ConnectionOutput::SendData(encoded));
    }
}
```

各 Connection は `policy` を個別フィールドとして保持する。
構造体定義は Step 1 に記載済み（`WebSocketClientConnection` / `WebSocketServerConnection`）。

`SharedConnectionState` の各メソッドは `FramePolicy` を追加の引数として受け取る。
`&mut self.shared` と `&mut self.policy` は異なるフィールドのため同時借用が可能:

```rust
impl<R: RandomSource> WebSocketClientConnection<R> {
    fn feed_recv_buf(&mut self, buf: &[u8], now: Timestamp) -> Result<(), Error> {
        // &mut self.shared と &mut self.policy は別フィールド → 同時借用が可能
        self.shared.process_frames(buf, &mut self.policy)
    }
}
```

`SharedConnectionState` 内のメソッドは `policy` を伝搬する。
0020 で `close_internal` の戻り値型は `()` に統一されているため、
呼び出しに `?` は不要で `;` を使用する:

```rust
impl SharedConnectionState {
    fn handle_decoded_frame(
        &mut self,
        decoded: DecodedFrame,
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        // RFC 6455 Section 5.1: マスク方向検証
        // DecodedFrame.masked から取得し、Frame とは別に渡す
        if let Err(e) = policy.verify_frame_masking(decoded.masked) {
            self.close_internal(CloseCode::PROTOCOL_ERROR, "invalid frame masking", policy);
            return Err(e);
        }
        self.handle_frame(decoded.frame, policy)
    }

    fn process_frames(&mut self, buf: &[u8], policy: &mut impl FramePolicy) -> Result<(), Error> {
        self.frame_decoder.feed(buf);
        loop {
            match self.frame_decoder.decode_with_info() {
                Ok(Some(decoded)) => {
                    self.handle_decoded_frame(decoded, policy)?;
                }
                Ok(None) => break,
                Err(e) => {
                    self.close_internal(CloseCode::PROTOCOL_ERROR, "frame decode error", policy);
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    fn handle_frame(&mut self, frame: Frame, policy: &mut impl FramePolicy) -> Result<(), Error> {
        // ... 各種検証 ...
        self.handle_close(frame, policy)?;
        // ...
    }

    fn handle_close(&mut self, frame: Frame, policy: &mut impl FramePolicy) -> Result<(), Error> {
        // ... state 更新 ...
        policy.encode_and_send(&reply_frame, self);
        // ...
    }
}
```

`handle_decoded_frame` も `SharedConnectionState` に移動することで、
`process_frames` 内のループから直接呼び出せるようになる。

`handle_timer` 内の 2 つの変更点:

#### `TimerId::PongTimeout` の `close()` → `close_internal()` 置き換え

`close` は各 Connection の公開メソッドであり、SharedConnectionState から呼べないため
`close_internal` に置き換える。戻り値型が `()` のため `?` → `;` への変更も必要。
この置き換えによる動作の違いは以下の通り:

- `close`: `Disconnected`/`Closed` で呼ぶと `Err` を返す → `handle_timer` が `Err` を返す
- `close_internal`: 同条件で何もせず `()` を返す → `handle_timer` は `Ok(())` を返す

`handle_timer` は `Closing` 状態からの PongTimeout 発火で `close_internal` を呼ぶが、
既に `close_sent == true` であれば何も送信しない。`Disconnected`/`Closed` 状態では
PongTimeout タイマーは既にクリア済みのため発火しない。万一発火しても `close_internal` は
安全に無視する。既に `event_queue` に `Error("pong timeout")` が積まれているため、
外部 I/O ループはエラーを検出できる。

#### `TimerId::Ping` の Ping 送信ロジックのインライン化

`send_ping` は各 Connection に残留するため、`SharedConnectionState` の `handle_timer` から
直接呼び出せない。Ping 送信ロジックを `handle_timer` 内にインライン展開する:

```rust
TimerId::Ping => {
    if self.state == ConnectionState::Connected && !self.awaiting_pong {
        // RFC 6455 Section 5.5: コントロールフレームは 125 バイト以下。
        // 空ペイロードは 0 バイトのため unwrap() は安全
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

Connection 側の公開メソッド `send_ping` は任意ペイロードの Ping 送信を提供する
外部向け API であり、引き続き各 Connection に残留する。`send_ping` は `check_connected`
による state チェックを行うため `SharedConnectionState` のメソッドからは呼ばない。

### 補足: 共通化対象外のメソッド

重複テーブルに含まれる `close` と `send_ping` は `SharedConnectionState` に移動しない。
理由は以下の通り:

- `close`: state チェック (`Connected`/`Closing`) と close code の妥当性検証を行う公開 API。
  内部的な `close_internal` とは責務が異なるため各 Connection に残す。
- `send_ping`: `check_connected` と `Frame::ping()` の生成 + `awaiting_pong` 設定を行う薄いラッパー。
  `encode_and_send` 経由のフレーム送信に変更するが、メソッド自体は各 Connection に残す。

### 補足: `ClientFramePolicy` の `random` フィールドと `connect()` の nonce 生成

`random` が `ClientFramePolicy` に移動した後、`connect()` 内の nonce 生成は
`self.policy.random.nonce()` となる。`ClientFramePolicy` の `random` フィールドは
`pub(crate)` で公開する（Step 3 のコード例に反映済み）。

`FramePolicy` トレイトに `nonce` メソッドを追加する案は採用しない。
nonce 生成はクライアント固有の機能であり、サーバー側の `ServerFramePolicy` には不要。
トレイトに追加すると不要なメソッドの実装を強制することになる。

### 補足: Connection 側に残留するメソッドのフィールドアクセス変更

`SharedConnectionState` にフィールドが移動するため、Connection 側に残留する
メソッドのフィールドアクセスを `self.xxx` → `self.shared.xxx` に変更する必要がある。
対象は以下:

**client 側:**
- `connect()` (L398-440): `self.set_state(...)` → `self.shared.set_state(...)`、
  `self.output_queue` → `self.shared.output_queue`
- `complete_handshake()` (L694-845): `self.set_state(...)` → `self.shared.set_state(...)`、
  `self.event_queue` → `self.shared.event_queue`、`self.output_queue` → `self.shared.output_queue`、
  `self.deflate` → `self.shared.deflate`
- `process_handshake()` (L663-692): `self.process_frames(...)` →
  `self.shared.process_frames(..., &mut self.policy)`
- `feed_recv_buf()` (L446-461): `self.failed` → `self.shared.failed`、
  `self.process_frames(buf, now)` → `self.shared.process_frames(buf, &mut self.policy)`
- `send_text()` (L464-467) / `send_binary()` (L470-473):
  `self.check_connected()` → `self.shared.check_connected()`、
  `self.send_data_frame(...)` → `self.shared.send_data_frame(..., &mut self.policy)`
- `close()` (L526-557): `self.send_frame(frame)?;` → `self.policy.encode_and_send(&frame, &mut self.shared);`
  （`send_frame` は常に `Ok(())` を返すため `?` 除去で安全）
- `send_ping()` (L504-519): 同上

**server 側:**
- `accept_handshake()` (L318-541): 同パターンの置換。
  `self.process_frames(&pending)?;` → `self.shared.process_frames(&pending, &mut self.policy)?;`
- `reject_handshake()` (L548-581): `self.set_state(...)` → `self.shared.set_state(...)`、
  `self.output_queue` → `self.shared.output_queue`
- `process_handshake()` (L781-816): `self.set_state(...)` → `self.shared.set_state(...)`
- `feed_recv_buf()` (L264-281): `self.failed` → `self.shared.failed`、
  `self.process_frames(buf)` → `self.shared.process_frames(buf, &mut self.policy)`
- `send_text()` (L584-586) / `send_binary()` (L590-592):
  `self.check_connected()` → `self.shared.check_connected()`、
  `self.send_data_frame(...)` → `self.shared.send_data_frame(..., &mut self.policy)`
- `close()` (L647-678): `self.send_frame(frame);` → `self.policy.encode_and_send(&frame, &mut self.shared);`
- `send_ping()` (L625-640): 同上

**公開メソッドの委譲パターン:**

`poll_event` / `poll_output` は `SharedConnectionState` に移動するため、
各 Connection の公開メソッドは単純な委譲となる:

```rust
pub fn poll_event(&mut self) -> Option<ConnectionEvent> {
    self.shared.poll_event()
}

pub fn poll_output(&mut self) -> Option<ConnectionOutput> {
    self.shared.poll_output()
}
```

### 補足: `encode_unmasked` の `#[allow(dead_code)]` 除去

`src/websocket_frame.rs` の `encode_unmasked` には `#[allow(dead_code)]` が付与されているが、
現在既に `websocket_server_connection.rs:776` から呼び出されている。
リファクタリング後も `ServerFramePolicy::encode_and_send` から使用されるため、
この `#[allow(dead_code)]` を除去する。

### 補足: `handle_timer` の `now: Timestamp` パラメータ

`handle_timer` は client/server 両方で `now: Timestamp` を受け取っていない。
そのため `now` 削除対象の 7 メソッド一覧に含まれない。Step 2 で `SharedConnectionState`
に移動後もシグネチャ変更は発生しない。

### Step 4: 共通型を独立モジュールに移動する

- `ConnectionState`, `TimerId`, `ConnectionEvent`, `ConnectionOutput` を
  `src/connection_types.rs` に移動する
- `SharedConnectionState` の可視性は `pub(crate)` とし、`lib.rs` には re-export しない

影響ファイルと変更内容:

| ファイル | 変更内容 |
|---|---|
| `src/connection_types.rs` | 新規作成。4 型の定義を移動 |
| `src/connection_shared.rs` | 新規作成。`SharedConnectionState` + `FragmentBuffer` + `FramePolicy` + ポリシー構造体 |
| `src/websocket_client_connection.rs` | 4 型の定義を削除。`use crate::connection_types::*;` を追加 |
| `src/websocket_server_connection.rs` | `use crate::{ConnectionEvent, ...};` の参照先を `connection_types` に変更 |
| `src/lib.rs` | `mod connection_types;` と `mod connection_shared;` を追加。re-export 元を `connection_types` に変更 |

PBT テスト (`pbt/tests/prop_*.rs`) は `shiguredo_websocket::*` 経由で import しているため、
`lib.rs` の re-export が正しく更新されていればビルドに影響しない。

### 補足: `FragmentBuffer` の移動先

`FragmentBuffer` は現在 `websocket_client_connection.rs` と `websocket_server_connection.rs`
の両方で private 定義が重複している。共通化後は `src/connection_shared.rs` に
1 つだけ定義し、可視性は `pub(crate)` とする。フィールドアクセスは `SharedConnectionState`
経由のみ。

## テスト戦略

本リファクタリングは内部構造の変更であり外部 API は変わらない。
以下の手順でリグレッションがないことを確認する:

1. `cargo check --workspace` でコンパイルが通ることを確認する
   （use パス変更によるコンパイルエラーを最初に検出するため）
2. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する
3. `cargo test --workspace` が全件パスすること
4. 既存の PBT が全件パスすること（`cargo test -p pbt`）
5. 既存の Fuzzing ターゲットが panic しないこと
6. リファクタリング前後で既存 PBT strategy が内部フィールドに依存していないこと
   （`prop_client_connection.rs`, `prop_server_connection.rs` の strategy は公開 API
   のみを使用しており影響なし）

新モジュール (`connection_shared.rs`, `connection_types.rs`) は内部実装の移動であり、
新規テストは不要。既存テストが全て通過すればリグレッションなしと判断する。

## 後方互換性

- `DEFAULT_MAX_FRAME_SIZE`, `DEFAULT_MAX_MESSAGE_SIZE`, `DEFAULT_MAX_DECOMPRESSED_SIZE` は
  `websocket_client_connection` と `websocket_server_connection` の両方で `pub const` として
  公開されている。移動後は元の定義場所に `pub use crate::connection_shared::DEFAULT_MAX_*;` を
  追加して再 export する。`lib.rs` の re-export リスト (L88-91, L100) は
  `websocket_client_connection` / `websocket_server_connection` 経由のため変更不要。
  deprecated にはしない（単なる定義位置の移動のため）。
- 公開メソッド (`connect`, `send_text`, `send_binary`, `send_ping`, `close`, `handle_timer`,
  `poll_event`, `poll_output`) のシグネチャは変更しない

## CHANGES.md

実装後、`CHANGES.md` の `## develop` → `### misc` セクションに以下を追記する:

```
- [UPDATE] クライアント/サーバー間のフレーム処理ロジックの重複を共通化する
  - @実装者名
```
