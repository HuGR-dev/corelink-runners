//! Durable binding capture and restoration; never replace conflicting live evidence.
use super::*;

impl NoBoxProvisioner {
    pub(super) fn binding_ref(&self, lease_id: &str) -> Result<String> {
        if !self
            .leases
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(lease_id)
        {
            bail!("no-box provision evidence missing for {lease_id}");
        }
        ProviderBinding {
            lease_id: lease_id.into(),
            backend: ProviderBackend::NoBox,
            route: ProviderRoute::NoBox,
            handle: None,
            domain: "local:nobox".into(),
        }
        .encode()
    }

    pub(super) fn restore_binding(&self, lease_id: &str, provider_ref: &str) -> Result<()> {
        let b = ProviderBinding::decode(provider_ref)?;
        if b.lease_id != lease_id
            || b.backend != ProviderBackend::NoBox
            || b.route != ProviderRoute::NoBox
            || b.domain != "local:nobox"
        {
            bail!("invalid no-box provider binding")
        }
        self.leases
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(lease_id.into());
        Ok(())
    }
}

impl<H: corelink_cloud_engine::HttpTransport> NorthflankBoxProvisioner<H> {
    pub(super) fn binding_ref(&self, lease_id: &str) -> Result<String> {
        let c = self
            .registry
            .resolve(lease_id)
            .ok_or_else(|| anyhow::anyhow!("no provider handle for {lease_id}"))?;
        ProviderBinding {
            lease_id: lease_id.into(),
            backend: ProviderBackend::Northflank,
            route: self.registry.route(lease_id)?,
            handle: Some(c.name),
            domain: self.engine.provider_domain()?,
        }
        .encode()
    }

    pub(super) fn restore_binding(&self, lease_id: &str, provider_ref: &str) -> Result<()> {
        let b = ProviderBinding::decode(provider_ref)?;
        if b.lease_id != lease_id
            || b.backend != ProviderBackend::Northflank
            || b.domain != self.engine.provider_domain()?
            || b.handle.is_none()
        {
            bail!("provider binding domain/backend mismatch")
        }
        self.registry.bind_with_route(
            lease_id,
            RunningContainer {
                name: b
                    .handle
                    .ok_or_else(|| anyhow::anyhow!("missing provider handle"))?,
            },
            b.route,
        )
    }
}

impl<H: corelink_cloud_engine::HttpTransport> CloudflareBoxProvisioner<H> {
    pub(super) fn binding_ref(&self, lease_id: &str) -> Result<String> {
        let c = self.registry.resolve(lease_id);
        let (route, handle) = if let Some(c) = c {
            (self.registry.route(lease_id)?, Some(c.name))
        } else if self.registry.has_no_box(lease_id) {
            (ProviderRoute::NoBox, None)
        } else {
            bail!("no Cloudflare binding for {lease_id}")
        };
        let backend = if route == ProviderRoute::NoBox {
            ProviderBackend::NoBox
        } else {
            ProviderBackend::Cloudflare
        };
        let domain = if backend == ProviderBackend::NoBox {
            "local:nobox".into()
        } else {
            self.engine.provider_domain().into()
        };
        ProviderBinding {
            lease_id: lease_id.into(),
            backend,
            route,
            handle,
            domain,
        }
        .encode()
    }

    pub(super) fn restore_binding(&self, lease_id: &str, provider_ref: &str) -> Result<()> {
        let b = ProviderBinding::decode(provider_ref)?;
        if b.lease_id != lease_id {
            bail!("provider binding lease mismatch")
        }
        if b.backend == ProviderBackend::NoBox
            && b.domain == "local:nobox"
            && b.route == ProviderRoute::NoBox
        {
            self.registry.mark_no_box(lease_id)?;
            return Ok(());
        }
        if b.backend != ProviderBackend::Cloudflare || b.domain != self.engine.provider_domain() {
            bail!("provider binding domain/backend mismatch")
        }
        match (b.route, b.handle) {
            (ProviderRoute::Runner | ProviderRoute::CheckHost, Some(handle)) => self
                .registry
                .bind_with_route(lease_id, RunningContainer { name: handle }, b.route),
            _ => bail!("invalid Cloudflare provider route/handle"),
        }
    }
}

impl HybridBoxProvisioner {
    pub(super) fn binding_ref(&self, lease_id: &str) -> Result<String> {
        let route = self
            .route_of(lease_id)
            .ok_or_else(|| anyhow::anyhow!("no hybrid route for {lease_id}"))?;
        let sub = match route {
            HybridRoute::Runner | HybridRoute::CheckHost => &self.runner,
            HybridRoute::Check => &self.check,
        };
        let mut b = ProviderBinding::decode(&sub.provider_ref(lease_id)?)?;
        b.route = match route {
            HybridRoute::Runner => ProviderRoute::Runner,
            HybridRoute::CheckHost => ProviderRoute::CheckHost,
            HybridRoute::Check => ProviderRoute::Check,
        };
        b.encode()
    }

    pub(super) fn restore_binding(&self, lease_id: &str, provider_ref: &str) -> Result<()> {
        let b = ProviderBinding::decode(provider_ref)?;
        if b.lease_id != lease_id {
            bail!("provider binding lease mismatch")
        }
        let sub: &Arc<dyn BoxProvisioner> = match b.route {
            ProviderRoute::Runner | ProviderRoute::CheckHost => &self.runner,
            ProviderRoute::Check => &self.check,
            ProviderRoute::NoBox => bail!("hybrid cannot restore no-box binding"),
        };
        let route = match b.route {
            ProviderRoute::Runner => HybridRoute::Runner,
            ProviderRoute::CheckHost => HybridRoute::CheckHost,
            ProviderRoute::Check => HybridRoute::Check,
            ProviderRoute::NoBox => unreachable!(),
        };
        // Hold route ownership until subordinate atomic restore succeeds. Never
        // mutate either table when an existing route conflicts with the journal.
        let mut routes = self.routes.lock().unwrap_or_else(|p| p.into_inner());
        if routes.get(lease_id).is_some_and(|old| *old != route) {
            bail!("conflicting hybrid provider route");
        }
        sub.restore_provider_ref(lease_id, provider_ref)?;
        routes.insert(lease_id.into(), route);
        Ok(())
    }
}

impl BoxRegistry {
    pub(super) fn bind_with_route(
        &self,
        lease_id: &str,
        container: RunningContainer,
        route: ProviderRoute,
    ) -> Result<()> {
        let mut evidence = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if evidence.no_box.contains(lease_id)
            || evidence
                .boxes
                .get(lease_id)
                .is_some_and(|old| old.name != container.name)
            || evidence
                .modes
                .get(lease_id)
                .is_some_and(|old| *old != route)
        {
            bail!("conflicting local provider binding");
        }
        evidence.boxes.insert(lease_id.into(), container);
        evidence.modes.insert(lease_id.into(), route);
        Ok(())
    }

    pub(super) fn route(&self, lease_id: &str) -> Result<ProviderRoute> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .modes
            .get(lease_id)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("provider route missing"))
    }

    pub(super) fn check_mode(&self, lease_id: &str) -> Result<bool> {
        match self.route(lease_id)? {
            ProviderRoute::Runner => Ok(false),
            ProviderRoute::CheckHost => Ok(true),
            _ => bail!("invalid Cloudflare provider route"),
        }
    }
}
