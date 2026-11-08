//! Native outbound TLS policy. Provider identity never disables certificate checks.
use rustls_platform_verifier::BuilderVerifierExt;
use std::sync::Arc;
use stravia_vendor_sdk::transport::TransportProfile;
use tokio_rustls::rustls;

pub(super) fn configure(
    builder: reqwest::ClientBuilder,
    profile: &TransportProfile,
    http1_only: bool,
) -> anyhow::Result<reqwest::ClientBuilder> {
    if !profile.has_custom_tls() {
        return Ok(builder);
    }
    let mut config = tls_config(profile)?;
    config.alpn_protocols = if http1_only {
        vec![b"http/1.1".to_vec()]
    } else {
        vec![b"h2".to_vec(), b"http/1.1".to_vec()]
    };
    Ok(builder.tls_backend_preconfigured(config))
}

fn crypto_provider(profile: &TransportProfile) -> anyhow::Result<rustls::crypto::CryptoProvider> {
    let mut crypto = rustls::crypto::aws_lc_rs::default_provider();
    if !profile.cipher_suites.is_empty() {
        let available = crypto.cipher_suites.clone();
        crypto.cipher_suites = profile
            .cipher_suites
            .iter()
            .map(|name| {
                available
                    .iter()
                    .find(|suite| format!("{:?}", suite.suite()) == *name)
                    .copied()
                    .ok_or_else(|| {
                        anyhow::anyhow!("Configured TLS cipher is unavailable in this build")
                    })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
    }
    if !profile.key_exchange_groups.is_empty() {
        let available = crypto.kx_groups.clone();
        crypto.kx_groups = profile
            .key_exchange_groups
            .iter()
            .map(|name| {
                available
                    .iter()
                    .find(|group| format!("{:?}", group.name()) == *name)
                    .copied()
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Configured TLS key exchange group is unavailable in this build"
                        )
                    })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
    }
    Ok(crypto)
}

fn tls_config(profile: &TransportProfile) -> anyhow::Result<rustls::ClientConfig> {
    let versions = [
        (&rustls::version::TLS13, "1.3"),
        (&rustls::version::TLS12, "1.2"),
    ]
    .into_iter()
    .filter(|(_, version)| {
        *version >= profile.min_version.as_deref().unwrap_or("1.2")
            && *version <= profile.max_version.as_deref().unwrap_or("1.3")
    })
    .map(|(version, _)| version)
    .collect::<Vec<_>>();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(crypto_provider(profile)?))
        .with_protocol_versions(&versions)?
        .with_platform_verifier()?
        .with_no_client_auth();
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    async fn vendor_transport_sends_configured_clienthello_for_https_and_wss() -> anyhow::Result<()>
    {
        use tokio::io::AsyncReadExt;
        for http1 in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let address = listener.local_addr()?;
            let server = async {
                let (mut stream, _) = listener.accept().await?;
                let mut acceptor = rustls::server::Acceptor::default();
                let mut bytes = [0; 16384];
                loop {
                    let n = stream.read(&mut bytes).await?;
                    anyhow::ensure!(n > 0, "TLS client closed before ClientHello");
                    acceptor.read_tls(&mut std::io::Cursor::new(&bytes[..n]))?;
                    if let Some(accepted) = acceptor
                        .accept()
                        .map_err(|_| anyhow::anyhow!("Invalid ClientHello"))?
                    {
                        let hello = accepted.client_hello();
                        assert_eq!(
                            hello.cipher_suites()[0],
                            rustls::CipherSuite::TLS13_CHACHA20_POLY1305_SHA256
                        );
                        assert_eq!(
                            hello.cipher_suites()[1],
                            rustls::CipherSuite::TLS13_AES_128_GCM_SHA256
                        );
                        assert_eq!(hello.named_groups().unwrap()[0], rustls::NamedGroup::X25519);
                        let alpn = hello.alpn().unwrap().collect::<Vec<_>>();
                        if http1 {
                            assert_eq!(alpn, [b"http/1.1".as_slice()]);
                        } else {
                            assert_eq!(alpn, [b"h2".as_slice(), b"http/1.1".as_slice()]);
                        }
                        return Ok::<_, anyhow::Error>(());
                    }
                }
            };
            let profile = TransportProfile::from_options(&serde_json::from_value(json!({
                "tls_min_version":"1.3",
                "tls_cipher_suites":"TLS13_CHACHA20_POLY1305_SHA256,TLS13_AES_128_GCM_SHA256",
                "tls_key_exchange_groups":"X25519,secp256r1"
            }))?)
            .map_err(anyhow::Error::msg)?;
            let mut builder = configure(reqwest::Client::builder(), &profile, http1)?.no_proxy();
            if http1 {
                builder = builder.http1_only();
            }
            let client = builder.build()?;
            let request = async {
                assert!(
                    client
                        .get(format!("https://{address}/"))
                        .send()
                        .await
                        .is_err()
                );
            };
            let (result, _) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                tokio::join!(server, request)
            })
            .await?;
            result?;
        }
        Ok(())
    }

    #[test]
    fn vendor_transport_retains_configured_cipher_and_group_order() -> anyhow::Result<()> {
        let options = serde_json::from_value(json!({
            "tls_min_version":"1.3",
            "tls_cipher_suites":"TLS13_CHACHA20_POLY1305_SHA256,TLS13_AES_128_GCM_SHA256",
            "tls_key_exchange_groups":"X25519,secp256r1"
        }))?;
        let profile = TransportProfile::from_options(&options).map_err(anyhow::Error::msg)?;
        let crypto = crypto_provider(&profile)?;
        assert_eq!(
            format!("{:?}", crypto.cipher_suites[0].suite()),
            "TLS13_CHACHA20_POLY1305_SHA256"
        );
        assert_eq!(format!("{:?}", crypto.kx_groups[0].name()), "X25519");
        configure(reqwest::Client::builder(), &profile, false)?.build()?;
        Ok(())
    }
}
