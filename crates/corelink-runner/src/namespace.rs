//! CoreLink-owned runtime namespaces.
//!
//! Every runtime resource name, label, workspace path, census filter, and
//! teardown sweep must use these constants.  Keeping the lifecycle vocabulary
//! in one module prevents a create/sweep mismatch from stranding resources on
//! a shared runner box.

/// Prefix for the single-job (C2a) container namespace.
pub const JOB_PREFIX: &str = "corelink-job-";
/// Prefix for the concurrent-job (C2b) container namespace.
pub const C2B_PREFIX: &str = "corelink-c2b-";
/// Prefix for the sparse-fence (C5a) container namespace.
pub const C5A_PREFIX: &str = "corelink-c5a-";
/// Prefix for the escape red-team (C5b) container namespace.
pub const C5B_PREFIX: &str = "corelink-c5b-";
/// Prefix for workspace (C9) containers.
pub const WS_PREFIX: &str = "corelink-ws-";

/// Common job-container label.
pub const JOB_LABEL: &str = "corelink.job=1";
/// C5a acceptance-container label.
pub const C5A_LABEL: &str = "corelink.wp=c5a";
/// C5b acceptance-container label.
pub const C5B_LABEL: &str = "corelink.wp=c5b";

/// Private tmpfs root for ordinary jobs.
pub const JOB_TMP_ROOT: &str = "/corelink/tmp";
/// Private tmpfs root for the sparse-fence acceptance lane.
pub const C5A_WORKSPACE_ROOT: &str = "/corelink-c5a-ws";
/// Private tmpfs root for the escape red-team acceptance lane.
pub const C5B_WORKSPACE_ROOT: &str = "/corelink-c5b-ws";

/// Migration-only cleanup vocabulary for resources created before the
/// CoreLink namespace cutover.  These values are intentionally isolated from
/// all creation, census, and normal teardown paths.  A future one-shot box
/// migration may consume this mapping and then remove it.
#[allow(dead_code)]
pub const HISTORICAL_CLEANUP_PREFIXES: &[&str] =
    &["hugit-job-", "hugit-c2b-", "hugit-c5a-", "hugit-c5b-"];

const ACTIVE_PREFIXES: &[&str] = &[JOB_PREFIX, C2B_PREFIX, C5A_PREFIX, C5B_PREFIX, WS_PREFIX];

/// Return whether a Docker/provider resource name belongs to a current
/// CoreLink namespace. The delimiter is part of every prefix, so a name such
/// as `corelink-job-foreign` is owned while `corelink-job` is not.
#[must_use]
pub fn is_corelink_owned_name(name: &str) -> bool {
    ACTIVE_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix) && name.len() > prefix.len())
}

/// Validate a resource name immediately before a provider creates it.
pub fn validate_corelink_owned_name(name: &str) -> anyhow::Result<()> {
    if !is_corelink_owned_name(name)
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
    {
        anyhow::bail!(
            "refusing to create resource {name:?}: name is outside a CoreLink-owned namespace"
        );
    }
    Ok(())
}

/// Filter a provider/container listing to names eligible for the one-shot
/// legacy migration cleanup. It consumes the historical mapping above and
/// applies an exact prefix check before any caller can delete a name.
pub fn historical_cleanup_names(listing: &str) -> impl Iterator<Item = &str> {
    listing.lines().map(str::trim).filter(|name| {
        HISTORICAL_CLEANUP_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix) && name.len() > prefix.len())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_namespaces_are_corelink_owned_and_disjoint() {
        let prefixes = [JOB_PREFIX, C2B_PREFIX, C5A_PREFIX, C5B_PREFIX, WS_PREFIX];
        for prefix in prefixes {
            assert!(prefix.starts_with("corelink-"));
            assert!(!prefix.contains("hugit"));
        }
        for (index, prefix) in prefixes.iter().enumerate() {
            assert!(
                prefixes[index + 1..]
                    .iter()
                    .all(|other| !other.starts_with(prefix))
            );
        }
        assert!(!JOB_LABEL.contains("hugit"));
        assert!(!C5A_LABEL.contains("hugit"));
        assert!(!C5B_LABEL.contains("hugit"));
        assert!(!JOB_TMP_ROOT.starts_with("/hugit"));
        assert!(!C5A_WORKSPACE_ROOT.starts_with("/hugit"));
        assert!(!C5B_WORKSPACE_ROOT.starts_with("/hugit"));
    }

    #[test]
    fn historical_cleanup_filter_is_prefix_scoped() {
        let names = "hugit-c2b-good\nmy-hugit-c2b-foreign\nhugit-c2b\n";
        assert_eq!(
            historical_cleanup_names(names).collect::<Vec<_>>(),
            ["hugit-c2b-good"]
        );
    }

    #[test]
    fn resource_name_validation_rejects_legacy_and_foreign_names() {
        assert!(validate_corelink_owned_name("corelink-job-abc").is_ok());
        assert!(validate_corelink_owned_name("hugit-job-abc").is_err());
        assert!(validate_corelink_owned_name("githugr-job-abc").is_err());
        assert!(validate_corelink_owned_name("runner-job").is_err());
        assert!(validate_corelink_owned_name("corelink-job-abc/evil").is_err());
    }
}
