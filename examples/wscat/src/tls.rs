//! rustls の `ClientConfig` を CLI オプションから組み立てる

use std::path::Path;
use std::sync::Arc;

use rustls::ClientConfig;
use rustls::RootCertStore;
use rustls::client::danger::{ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls_platform_verifier::ConfigVerifierExt;

use crate::cli::Cli;
use crate::common::AnyError;

/// CLI から `ClientConfig` を組み立てる
pub fn build_client_config(cli: &Cli) -> Result<ClientConfig, AnyError> {
    // クライアント認証の整合性チェック
    if cli.cert.is_some() != cli.key.is_some() {
        return Err("--cert and --key must be specified together".into());
    }

    // サーバー証明書検証の方針
    let builder = ClientConfig::builder();

    let builder = if cli.no_check {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(InsecureVerifier))
    } else if let Some(ca_path) = cli.ca.as_ref() {
        let mut store = RootCertStore::empty();
        let certs = load_certs(ca_path)?;
        for cert in certs {
            store
                .add(cert)
                .map_err(|e| format!("failed to load CA certificate: {}", e))?;
        }
        builder.with_root_certificates(Arc::new(store))
    } else {
        return Ok(ClientConfig::with_platform_verifier()
            .map_err(|e| format!("failed to load platform verifier: {}", e))?);
    };

    let config = match (cli.cert.as_deref(), cli.key.as_deref()) {
        (Some(cert), Some(key)) => {
            let certs = load_certs(cert)?;
            let key = PrivateKeyDer::from_pem_file(key)
                .map_err(|e| format!("failed to load client key: {}", e))?;
            builder
                .with_client_auth_cert(certs, key)
                .map_err(|e| format!("failed to build TLS config: {}", e))?
        }
        _ => builder.with_no_client_auth(),
    };

    Ok(config)
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, AnyError> {
    let certs: Vec<_> = CertificateDer::pem_file_iter(path)
        .map_err(|e| format!("failed to open certificate file {:?}: {}", path, e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to parse certificate file {:?}: {}", path, e))?;
    if certs.is_empty() {
        return Err(format!("no certificates found in {:?}", path).into());
    }
    Ok(certs)
}

/// `--no-check` 用の検証器
#[derive(Debug)]
struct InsecureVerifier;

impl ServerCertVerifier for InsecureVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer,
        _intermediates: &[CertificateDer],
        _server_name: &ServerName,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
        ]
    }
}
