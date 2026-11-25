.PHONY: test test-e2e cover pbt pbt-cover fuzzing fuzzing-list check clippy fmt fmt-check clean

# Docker を必要としない全テストを実行する (e2e テストは GitHub Actions でだけ実行する)
test:
	cargo test --workspace --exclude websocket_client

# Docker を必要とする e2e テストを実行する
test-e2e:
	cargo test -p websocket_client

# ライブラリと PBT のテストカバレッジを取得する
cover:
	cargo llvm-cov --tests --workspace --exclude websocket_client

# PBT を実行する
pbt:
	cargo test -p pbt

# PBT をカバレッジ付きで実行する
pbt-cover:
	cargo llvm-cov -p pbt --tests

# Fuzzing を全ターゲットで 30 秒ずつ実行する
fuzzing:
	@for target in $$(cargo fuzz list); do \
		echo "=== Fuzzing $$target ==="; \
		cargo +nightly fuzz run $$target -- -max_total_time=30 || exit 1; \
	done

# Fuzzing ターゲット一覧を表示する
fuzzing-list:
	cargo fuzz list

# cargo check を実行する
check:
	cargo check --workspace --all-targets

# cargo clippy を実行する
clippy:
	cargo clippy --workspace --all-targets -- -D warnings

# フォーマットを適用する (workspace から除外されている fuzz も対象にする)
fmt:
	cargo fmt --all
	cargo fmt --manifest-path fuzz/Cargo.toml

# フォーマットの差分がないか確認する
fmt-check:
	cargo fmt --all -- --check
	cargo fmt --manifest-path fuzz/Cargo.toml -- --check

# ビルド成果物を削除する
clean:
	cargo clean
	cargo clean --manifest-path fuzz/Cargo.toml
