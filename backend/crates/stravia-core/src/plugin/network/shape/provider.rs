//! Apply administrator TLS choices without replacing the official transport.
use super::*;
use stravia_vendor_sdk::transport::TransportProfile;

impl ShapeRegistry {
    pub(crate) fn with_transport_profile(
        self: &Arc<Self>,
        profile: &TransportProfile,
    ) -> Result<Arc<Self>, ShapeError> {
        if !profile.has_custom_tls() {
            return Ok(Arc::clone(self));
        }
        let mut cache = self.profiles.lock();
        if let Some((_, registry)) = cache.iter().find(|(key, _)| key == profile) {
            return Ok(Arc::clone(registry));
        }
        let fingerprint = serde_json::to_string(profile)
            .map_err(|error| ShapeError::field("provider_tls", error))?;
        let crypto = rustls::crypto::aws_lc_rs::default_provider();
        let mut shapes = HashMap::new();
        for (name, original) in &self.shapes {
            let mut shape = original.declaration.clone();
            shape.source = None;
            let tls = &mut shape.tls;
            if profile.min_version.is_some() || profile.max_version.is_some() {
                tls.versions = ["1.2", "1.3"]
                    .into_iter()
                    .filter(|version| {
                        *version >= profile.min_version.as_deref().unwrap_or("1.2")
                            && *version <= profile.max_version.as_deref().unwrap_or("1.3")
                    })
                    .map(str::to_owned)
                    .collect();
            }
            if !profile.cipher_suites.is_empty() {
                tls.cipher_suites = profile
                    .cipher_suites
                    .iter()
                    .map(|name| {
                        crypto
                            .cipher_suites
                            .iter()
                            .find(|suite| format!("{:?}", suite.suite()) == *name)
                            .map(|suite| format!("0x{:04x}", u16::from(suite.suite())))
                            .ok_or_else(|| {
                                ShapeError::field("cipher_suites", "configured cipher unavailable")
                            })
                    })
                    .collect::<Result<_, _>>()?;
                tls.advertise_only_cipher_suites.clear();
            }
            if !profile.key_exchange_groups.is_empty() {
                tls.groups = profile.key_exchange_groups.clone();
                // Captured key shares cannot override an administrator's order.
                tls.key_shares.clear();
            }
            if profile.max_version.as_deref() == Some("1.2") {
                tls.key_shares.clear();
            }
            let identity = format!("{}:provider:{}", original.identity, fingerprint);
            shapes.insert(
                name.clone(),
                Arc::new(Self::compile(&shape, identity, &self.verifier)?),
            );
        }
        let registry = Arc::new(Self {
            shapes,
            profiles: Default::default(),
            proxy_tls: Arc::clone(&self.proxy_tls),
            verifier: Arc::clone(&self.verifier),
        });
        // Bound session/config residency independently of the number of edits.
        if cache.len() == 4 {
            cache.remove(0);
        }
        cache.push((profile.clone(), Arc::clone(&registry)));
        Ok(registry)
    }
}
