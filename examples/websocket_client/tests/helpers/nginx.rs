//! testcontainers ベースの nginx WebSocket プロキシヘルパー

use testcontainers::core::{Host, IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

const NGINX_READY_LOG: &str = "start worker processes";
const NGINX_IMAGE_NAME: &str = "nginx";
const NGINX_IMAGE_TAG: &str = "1.27-alpine";
const NGINX_INTERNAL_PORT: u16 = 80;

/// 起動済み nginx コンテナへのハンドル
pub struct NginxHandle {
    _container: ContainerAsync<GenericImage>,
    pub port: u16,
}

/// echo サーバーへの WebSocket プロキシとして nginx を起動する
pub async fn spawn_ws_proxy(echo_server_port: u16) -> NginxHandle {
    let conf = format!(
        r#"server {{
    listen 80;
    location / {{
        proxy_pass http://host.docker.internal:{echo_server_port};
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_read_timeout 10s;
    }}
}}"#
    );

    let request = GenericImage::new(NGINX_IMAGE_NAME, NGINX_IMAGE_TAG)
        .with_exposed_port(NGINX_INTERNAL_PORT.tcp())
        .with_wait_for(WaitFor::message_on_either_std(NGINX_READY_LOG))
        .with_copy_to("/etc/nginx/conf.d/default.conf", conf.into_bytes())
        .with_host("host.docker.internal", Host::HostGateway);

    let container = request
        .start()
        .await
        .expect("failed to start nginx container");
    let port = container
        .get_host_port_ipv4(NGINX_INTERNAL_PORT)
        .await
        .expect("failed to get nginx host port");

    NginxHandle {
        _container: container,
        port,
    }
}
