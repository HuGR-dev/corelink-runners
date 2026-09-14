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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_namespaces_are_corelink_owned_and_disjoint() {
        let prefixes = [JOB_PREFIX, C2B_PREFIX, C5A_PREFIX, C5B_PREFIX, WS_PREFIX];
        for prefix in prefixes {
            assert!(prefix.starts_with("corelink-"));
            assert!(!prefix.contains("legacy"));
        }
        for (index, prefix) in prefixes.iter().enumerate() {
            assert!(
                prefixes[index + 1..]
                    .iter()
                    .all(|other| !other.starts_with(prefix))
            );
        }
        assert!(!JOB_LABEL.contains("legacy"));
        assert!(!C5A_LABEL.contains("legacy"));
        assert!(!C5B_LABEL.contains("legacy"));
        assert!(!JOB_TMP_ROOT.starts_with("/legacy"));
        assert!(!C5A_WORKSPACE_ROOT.starts_with("/legacy"));
        assert!(!C5B_WORKSPACE_ROOT.starts_with("/legacy"));
    }

    #[test]
    fn resource_name_validation_rejects_legacy_and_foreign_names() {
        assert!(validate_corelink_owned_name("corelink-job-abc").is_ok());
        assert!(validate_corelink_owned_name("legacy-job-abc").is_err());
        assert!(validate_corelink_owned_name("foreign-job-abc").is_err());
        assert!(validate_corelink_owned_name("runner-job").is_err());
        assert!(validate_corelink_owned_name("corelink-job-abc/evil").is_err());
    }
}
