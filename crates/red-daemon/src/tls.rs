//! Local certificate authority and per-site certificates.
//!
//! RED creates its own CA once (`<data>/certs/ca.pem`). The CA carries an
//! X.509 Name Constraints extension that only permits the local TLD and
//! `localhost`, so even if its key leaked it could not be used to
//! impersonate real websites. Each secured site gets a leaf certificate for
//! `<name>.<tld>`, `*.<name>.<tld>` and the same under `localhost`.

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose,
    GeneralSubtree, IsCa, Issuer, KeyPair, KeyUsagePurpose, NameConstraints,
};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use time::{Duration, OffsetDateTime};

/// Lifetime of site certificates. Apple platforms reject server
/// certificates valid for more than 398 days.
const LEAF_DAYS: i64 = 397;
/// Reissue a site certificate when it has less than this left.
const RENEW_BEFORE_DAYS: i64 = 30;
/// Lifetime of the CA.
const CA_DAYS: i64 = 3650;
const CA_COMMON_NAME: &str = "RED Project Local CA";

/// Errors from creating or loading certificates.
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("certificate error: {0}")]
    Rcgen(#[from] rcgen::Error),
    #[error("could not read {path}: {reason}")]
    Pem { path: PathBuf, reason: String },
    #[error("TLS setup failed: {0}")]
    Rustls(#[from] rustls::Error),
}

fn io(context: impl Into<String>) -> impl FnOnce(std::io::Error) -> TlsError {
    let context = context.into();
    move |source| TlsError::Io { context, source }
}

/// The local CA plus a cache of loaded site certificates.
pub struct Authority {
    dir: PathBuf,
    tld: String,
    ca_key: KeyPair,
    cache: Mutex<HashMap<String, Arc<CertifiedKey>>>,
}

impl std::fmt::Debug for Authority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authority")
            .field("dir", &self.dir)
            .field("tld", &self.tld)
            .finish_non_exhaustive()
    }
}

impl Authority {
    /// Load the CA from `dir`, creating it on first use.
    pub fn load_or_create(dir: &Path, tld: &str) -> Result<Self, TlsError> {
        std::fs::create_dir_all(dir.join("sites"))
            .map_err(io(format!("create {}", dir.display())))?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(io(format!("chmod {}", dir.display())))?;

        let key_path = dir.join("ca-key.pem");
        let cert_path = dir.join("ca.pem");
        let ca_key = if key_path.exists() && cert_path.exists() {
            let pem = std::fs::read_to_string(&key_path)
                .map_err(io(format!("read {}", key_path.display())))?;
            KeyPair::from_pem(&pem)?
        } else {
            let key = KeyPair::generate()?;
            let cert = ca_params(tld)?.self_signed(&key)?;
            write_private(&key_path, &key.serialize_pem())?;
            std::fs::write(&cert_path, cert.pem())
                .map_err(io(format!("write {}", cert_path.display())))?;
            tracing::info!(path = %cert_path.display(), "created local certificate authority");
            key
        };
        Ok(Self {
            dir: dir.to_owned(),
            tld: tld.to_owned(),
            ca_key,
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// Path of the CA certificate users add to their trust store.
    pub fn ca_cert_path(&self) -> PathBuf {
        self.dir.join("ca.pem")
    }

    /// Issue (or reissue) the certificate for `site`. Returns its expiry as
    /// Unix time.
    pub fn issue(&self, site: &str) -> Result<u64, TlsError> {
        let now = OffsetDateTime::now_utc();
        let not_after = now + Duration::days(LEAF_DAYS);
        let names = vec![
            format!("{site}.{}", self.tld),
            format!("*.{site}.{}", self.tld),
            format!("{site}.localhost"),
            format!("*.{site}.localhost"),
        ];
        let mut params = CertificateParams::new(names)?;
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, format!("{site}.{}", self.tld));
        params.not_before = now - Duration::days(1);
        params.not_after = not_after;
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.use_authority_key_identifier_extension = true;

        let key = KeyPair::generate()?;
        let issuer = Issuer::new(ca_params(&self.tld)?, &self.ca_key);
        let cert = params.signed_by(&key, &issuer)?;

        let (cert_path, key_path) = self.site_paths(site);
        write_private(&key_path, &key.serialize_pem())?;
        std::fs::write(&cert_path, cert.pem())
            .map_err(io(format!("write {}", cert_path.display())))?;
        self.cache.lock().expect("cache poisoned").remove(site);
        Ok(not_after.unix_timestamp().max(0) as u64)
    }

    /// Delete the certificate for `site`.
    pub fn remove(&self, site: &str) {
        let (cert_path, key_path) = self.site_paths(site);
        let _ = std::fs::remove_file(cert_path);
        let _ = std::fs::remove_file(key_path);
        self.cache.lock().expect("cache poisoned").remove(site);
    }

    /// The certificate for `site`, loaded from disk once and then cached.
    pub fn certified_key(&self, site: &str) -> Option<Arc<CertifiedKey>> {
        if let Some(key) = self.cache.lock().expect("cache poisoned").get(site) {
            return Some(key.clone());
        }
        match self.load(site) {
            Ok(key) => {
                let key = Arc::new(key);
                self.cache
                    .lock()
                    .expect("cache poisoned")
                    .insert(site.to_owned(), key.clone());
                Some(key)
            }
            Err(err) => {
                tracing::warn!(site, %err, "no usable certificate");
                None
            }
        }
    }

    /// Whether the certificate whose expiry is `expires` should be renewed.
    pub fn needs_renewal(expires: u64) -> bool {
        let renew_at = OffsetDateTime::now_utc() + Duration::days(RENEW_BEFORE_DAYS);
        (expires as i64) < renew_at.unix_timestamp()
    }

    fn load(&self, site: &str) -> Result<CertifiedKey, TlsError> {
        let (cert_path, key_path) = self.site_paths(site);
        let pem_err = |path: &Path| {
            let path = path.to_owned();
            move |err: rustls_pki_types::pem::Error| TlsError::Pem {
                path,
                reason: err.to_string(),
            }
        };
        let cert = CertificateDer::from_pem_file(&cert_path).map_err(pem_err(&cert_path))?;
        let key = PrivateKeyDer::from_pem_file(&key_path).map_err(pem_err(&key_path))?;
        let provider = rustls::crypto::ring::default_provider();
        let signing_key = provider.key_provider.load_private_key(key)?;
        Ok(CertifiedKey::new(vec![cert], signing_key))
    }

    fn site_paths(&self, site: &str) -> (PathBuf, PathBuf) {
        let sites = self.dir.join("sites");
        (
            sites.join(format!("{site}.pem")),
            sites.join(format!("{site}-key.pem")),
        )
    }
}

/// Parameters of the CA certificate. Rebuilt identically each time so site
/// certificates name the right issuer.
fn ca_params(tld: &str) -> Result<CertificateParams, TlsError> {
    let now = OffsetDateTime::now_utc();
    let mut params = CertificateParams::new(Vec::<String>::new())?;
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, CA_COMMON_NAME);
    params
        .distinguished_name
        .push(DnType::OrganizationName, "RED Project");
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    params.name_constraints = Some(NameConstraints {
        permitted_subtrees: vec![
            GeneralSubtree::DnsName(tld.to_owned()),
            GeneralSubtree::DnsName("localhost".to_owned()),
        ],
        excluded_subtrees: Vec::new(),
    });
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(CA_DAYS);
    Ok(params)
}

fn write_private(path: &Path, contents: &str) -> Result<(), TlsError> {
    std::fs::write(path, contents).map_err(io(format!("write {}", path.display())))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(io(format!("chmod {}", path.display())))
}

/// Picks the certificate for each TLS handshake from the SNI name.
pub struct SiteResolver {
    pub app: Arc<crate::AppState>,
}

impl std::fmt::Debug for SiteResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SiteResolver")
    }
}

impl ResolvesServerCert for SiteResolver {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let sni = hello.server_name()?;
        let state = self.app.state_snapshot();
        let site = red_core::sites::resolve_host(&state, sni, &self.app.config.tld)?;
        if !state.secured.contains_key(&site.name) {
            tracing::debug!(sni, site = %site.name, "TLS handshake for a site that is not secured");
            return None;
        }
        self.app.tls.certified_key(&site.name)
    }
}

/// rustls server configuration that serves each secured site its own
/// certificate and offers HTTP/2.
pub fn server_config(app: Arc<crate::AppState>) -> Result<rustls::ServerConfig, TlsError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(SiteResolver { app }));
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_ca_once_and_issues_site_certificates() {
        let dir = tempfile::tempdir().unwrap();
        let certs = dir.path().join("certs");
        let authority = Authority::load_or_create(&certs, "test").unwrap();
        let ca_pem = std::fs::read_to_string(authority.ca_cert_path()).unwrap();
        let key_mode = std::fs::metadata(certs.join("ca-key.pem"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(key_mode & 0o777, 0o600);

        // Loading again reuses the same CA.
        let again = Authority::load_or_create(&certs, "test").unwrap();
        assert_eq!(
            std::fs::read_to_string(again.ca_cert_path()).unwrap(),
            ca_pem
        );

        let expires = authority.issue("blog").unwrap();
        assert!(!Authority::needs_renewal(expires));
        assert!(authority.certified_key("blog").is_some());
        authority.remove("blog");
        assert!(authority.certified_key("blog").is_none());
    }

    #[test]
    fn renewal_threshold() {
        let soon = (OffsetDateTime::now_utc() + Duration::days(5)).unix_timestamp() as u64;
        assert!(Authority::needs_renewal(soon));
    }
}
