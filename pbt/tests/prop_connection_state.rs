//! `ConnectionState` の許可遷移表のプロパティベーステスト
//!
//! 目的:
//! - `can_transition_to` が許可遷移表と一致することを保証する
//! - 不正な遷移が許可されていないことを保証する

use proptest::prelude::*;
use shiguredo_websocket::ConnectionState;

/// `ConnectionState` の全バリアント
const STATES: [ConnectionState; 5] = [
    ConnectionState::Disconnected,
    ConnectionState::Connecting,
    ConnectionState::Connected,
    ConnectionState::Closing,
    ConnectionState::Closed,
];

/// 期待される許可遷移表 (`from`, `to`)。
/// 同一状態への遷移は別途扱うため含めない
const ALLOWED: &[(ConnectionState, ConnectionState)] = &[
    (ConnectionState::Disconnected, ConnectionState::Connecting),
    (ConnectionState::Connecting, ConnectionState::Connected),
    (ConnectionState::Connecting, ConnectionState::Closed),
    (ConnectionState::Connected, ConnectionState::Closing),
    (ConnectionState::Connected, ConnectionState::Closed),
    (ConnectionState::Closing, ConnectionState::Closed),
];

fn is_allowed(from: ConnectionState, to: ConnectionState) -> bool {
    from == to || ALLOWED.iter().any(|&(f, t)| f == from && t == to)
}

proptest! {
    /// 全ての (from, to) ペアについて `can_transition_to` が期待表と一致する
    #[test]
    fn prop_can_transition_to_matches_expected_table(
        from_idx in 0usize..STATES.len(),
        to_idx in 0usize..STATES.len(),
    ) {
        let from = STATES[from_idx];
        let to = STATES[to_idx];
        prop_assert_eq!(from.can_transition_to(to), is_allowed(from, to));
    }
}

#[test]
fn each_allowed_transition_entry_is_permitted() {
    for &(from, to) in ALLOWED {
        assert!(
            from.can_transition_to(to),
            "transition from {:?} to {:?} must be allowed",
            from,
            to
        );
    }
}

#[test]
fn representative_disallowed_transitions_are_denied() {
    let denied: &[(ConnectionState, ConnectionState)] = &[
        // 終端からの遷移は不可
        (ConnectionState::Closed, ConnectionState::Disconnected),
        (ConnectionState::Closed, ConnectionState::Connecting),
        (ConnectionState::Closed, ConnectionState::Connected),
        (ConnectionState::Closed, ConnectionState::Closing),
        // 後退は不可
        (ConnectionState::Connecting, ConnectionState::Disconnected),
        (ConnectionState::Connected, ConnectionState::Connecting),
        (ConnectionState::Connected, ConnectionState::Disconnected),
        (ConnectionState::Closing, ConnectionState::Connected),
        (ConnectionState::Closing, ConnectionState::Connecting),
        (ConnectionState::Closing, ConnectionState::Disconnected),
        // ハンドシェイク前に確立や Closing には行けない
        (ConnectionState::Disconnected, ConnectionState::Connected),
        (ConnectionState::Disconnected, ConnectionState::Closing),
        (ConnectionState::Disconnected, ConnectionState::Closed),
        // Connecting から Closing へは Close フレーム送信を経由する必要があるため不可
        (ConnectionState::Connecting, ConnectionState::Closing),
    ];
    for &(from, to) in denied {
        assert!(
            !from.can_transition_to(to),
            "transition from {:?} to {:?} must be denied",
            from,
            to
        );
    }
}
