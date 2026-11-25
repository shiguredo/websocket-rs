//! e2e テストの共通ヘルパー

pub mod echo_server;
pub mod nginx;

use std::process::Stdio;

/// Docker daemon が応答するか確認する。無ければ panic で fail-fast する。
pub fn ensure_docker() {
    let status = std::process::Command::new("docker")
        .arg("version")
        .arg("--format")
        .arg("{{.Server.Version}}")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => {}
        _ => panic!("these integration tests require a running Docker daemon"),
    }
}
