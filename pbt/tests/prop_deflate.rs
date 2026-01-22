//! RFC 7692 permessage-deflate のプロパティベーステスト
//!
//! WebSocket 圧縮拡張 (RFC 7692) の準拠テスト

use proptest::prelude::*;
use shiguredo_websocket::PerMessageDeflateConfig;

// PerMessageDeflate は pub ではないため、Config 経由でテスト
// 内部テストは src/deflate/permessage_deflate.rs に追加

proptest! {
    // =========================================================================
    // RFC 7692 Section 7.1: Extension Negotiation
    // =========================================================================

    /// window_bits は 8-15 の範囲
    #[test]
    fn prop_window_bits_range(
        server_bits in 8u8..=15,
        client_bits in 8u8..=15
    ) {
        let config = PerMessageDeflateConfig::new()
            .server_max_window_bits(server_bits)
            .client_max_window_bits(client_bits);

        prop_assert_eq!(config.server_max_window_bits, Some(server_bits));
        prop_assert_eq!(config.client_max_window_bits, Some(client_bits));
    }

    /// window_bits が範囲外の場合はクランプ
    #[test]
    fn prop_window_bits_clamped(bits in 0u8..=20) {
        let config = PerMessageDeflateConfig::new()
            .server_max_window_bits(bits);

        let expected = bits.clamp(8, 15);
        prop_assert_eq!(config.server_max_window_bits, Some(expected));
    }

    /// no_context_takeover フラグの設定
    #[test]
    fn prop_no_context_takeover_flags(
        server_no_takeover in any::<bool>(),
        client_no_takeover in any::<bool>()
    ) {
        let mut config = PerMessageDeflateConfig::new();
        if server_no_takeover {
            config = config.server_no_context_takeover();
        }
        if client_no_takeover {
            config = config.client_no_context_takeover();
        }

        prop_assert_eq!(config.server_no_context_takeover, server_no_takeover);
        prop_assert_eq!(config.client_no_context_takeover, client_no_takeover);
    }

    // =========================================================================
    // Extension ヘッダー生成・パース
    // =========================================================================

    /// Extension ヘッダーのラウンドトリップ
    #[test]
    fn prop_extension_roundtrip(
        server_bits in prop::option::of(8u8..=15),
        client_bits in prop::option::of(8u8..=15),
        server_no_takeover in any::<bool>(),
        client_no_takeover in any::<bool>()
    ) {
        let mut config = PerMessageDeflateConfig::new();
        if let Some(bits) = server_bits {
            config = config.server_max_window_bits(bits);
        }
        if let Some(bits) = client_bits {
            config = config.client_max_window_bits(bits);
        }
        if server_no_takeover {
            config = config.server_no_context_takeover();
        }
        if client_no_takeover {
            config = config.client_no_context_takeover();
        }

        let ext = config.to_extension();
        let parsed = PerMessageDeflateConfig::from_extension(&ext).unwrap();

        prop_assert_eq!(parsed.server_no_context_takeover, server_no_takeover);
        prop_assert_eq!(parsed.client_no_context_takeover, client_no_takeover);

        if let Some(bits) = server_bits {
            prop_assert_eq!(parsed.server_max_window_bits, Some(bits));
        }
        if let Some(bits) = client_bits {
            prop_assert_eq!(parsed.client_max_window_bits, Some(bits));
        }
    }

    /// Extension 名は常に "permessage-deflate"
    #[test]
    fn prop_extension_name(
        server_bits in prop::option::of(8u8..=15)
    ) {
        let mut config = PerMessageDeflateConfig::new();
        if let Some(bits) = server_bits {
            config = config.server_max_window_bits(bits);
        }

        let ext = config.to_extension();
        prop_assert_eq!(ext.name, "permessage-deflate");
    }
}

// =============================================================================
// 圧縮/解凍テスト（内部モジュールへのアクセスが必要）
// =============================================================================

#[cfg(test)]
mod compression_tests {
    use super::*;

    proptest! {
        /// デフォルト設定での Config 生成
        #[test]
        fn prop_default_config(_dummy in 0u8..1) {
            let config = PerMessageDeflateConfig::default();
            prop_assert!(!config.server_no_context_takeover);
            prop_assert!(!config.client_no_context_takeover);
            prop_assert!(config.server_max_window_bits.is_none());
            prop_assert!(config.client_max_window_bits.is_none());
        }

        /// Clone の一貫性
        #[test]
        fn prop_config_clone(
            server_bits in prop::option::of(8u8..=15),
            client_bits in prop::option::of(8u8..=15),
            server_no_takeover in any::<bool>(),
            client_no_takeover in any::<bool>()
        ) {
            let mut config = PerMessageDeflateConfig::new();
            if let Some(bits) = server_bits {
                config = config.server_max_window_bits(bits);
            }
            if let Some(bits) = client_bits {
                config = config.client_max_window_bits(bits);
            }
            if server_no_takeover {
                config = config.server_no_context_takeover();
            }
            if client_no_takeover {
                config = config.client_no_context_takeover();
            }

            let cloned = config.clone();
            prop_assert_eq!(cloned.server_max_window_bits, config.server_max_window_bits);
            prop_assert_eq!(cloned.client_max_window_bits, config.client_max_window_bits);
            prop_assert_eq!(cloned.server_no_context_takeover, config.server_no_context_takeover);
            prop_assert_eq!(cloned.client_no_context_takeover, config.client_no_context_takeover);
        }
    }
}
