//! TLS for the remote gateway: a configured certificate or a self-signed one that clients pin
//! by its SHA-256 fingerprint.

use super::devices::sha256_hex;
use anyhow::{anyhow, Context, Result};
use openshell_config::Project;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::path::Path;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

pub const CERT_FILE: &str = "remote-cert.pem";
pub const KEY_FILE: &str = "remote-key.pem";

#[derive(Clone)]
pub struct Tls {
    pub acceptor: TlsAcceptor,
    /// SHA-256 of the leaf certificate, `AB:CD:…`.
    pub fingerprint: String,
    pub self_signed: bool,
}

/// Format a hex digest as `AB:CD:…`.
pub fn colon_hex(hex: &str) -> String {
    hex.as_bytes().chunks(2).map(|c| String::from_utf8_lossy(c).to_uppercase()).collect::<Vec<_>>().join(":")
}

/// Load the configured certificate, or create / reuse a self-signed one in the data directory.
pub fn load(project: &Project, hosts: &[String]) -> Result<Tls> {
    let cfg = &project.config.runtime.remote.tls;
    let (cert_path, key_path, self_signed) = match (&cfg.cert, &cfg.key) {
        (Some(c), Some(k)) => (project.root.join(c), project.root.join(k), false),
        _ => {
            let dir = project.data_dir();
            let (c, k) = (dir.join(CERT_FILE), dir.join(KEY_FILE));
            if !c.is_file() || !k.is_file() {
                generate(&dir, hosts)?;
            }
            (c, k, true)
        }
    };
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(&cert_path)
        .with_context(|| format!("cannot read {}", cert_path.display()))?
        .collect::<std::result::Result<_, _>>()
        .with_context(|| format!("invalid certificate in {}", cert_path.display()))?;
    let leaf = certs.first().ok_or_else(|| anyhow!("no certificate in {}", cert_path.display()))?;
    let fingerprint = colon_hex(&sha256_hex(leaf.as_ref()));
    let key = PrivateKeyDer::from_pem_file(&key_path)
        .with_context(|| format!("cannot read the private key {}", key_path.display()))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("certificate and key do not match")?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Tls { acceptor: TlsAcceptor::from(Arc::new(config)), fingerprint, self_signed })
}

/// A self-signed certificate for `localhost`, the device's addresses and its host name.
fn generate(dir: &Path, hosts: &[String]) -> Result<()> {
    let mut names: Vec<String> = vec!["localhost".into(), "127.0.0.1".into()];
    for h in hosts {
        if !names.contains(h) {
            names.push(h.clone());
        }
    }
    let cert = rcgen::generate_simple_self_signed(names).context("cannot create a TLS certificate")?;
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(CERT_FILE), cert.cert.pem())?;
    let key = dir.join(KEY_FILE);
    std::fs::write(&key, cert.key_pair.serialize_pem())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colon_hex_format() {
        assert_eq!(colon_hex("ab01ff"), "AB:01:FF");
    }

    #[test]
    fn self_signed_certificate_is_created_once() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("openshell.json"),
            r#"{ "version": 1, "app": { "name": "tls" }, "runtime": { "remote": { "enabled": true, "tls": { "enabled": true } } } }"#,
        )
        .unwrap();
        let project = Project::load(dir.path()).unwrap();
        let a = load(&project, &["192.168.1.9".into()]).unwrap();
        let b = load(&project, &[]).unwrap();
        assert!(a.self_signed);
        assert_eq!(a.fingerprint, b.fingerprint, "the certificate is reused");
        assert_eq!(a.fingerprint.len(), 32 * 3 - 1);
        assert!(project.data_dir().join(CERT_FILE).is_file());
    }
}
