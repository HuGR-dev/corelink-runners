//! Runner-mode box injection (ADR-0007 Stage A — the direct-CI ephemeral
//! GitHub Actions runner fleet). Inject the runner's JIT registration config
//! into the box env so `deploy/runner/entrypoint.sh` self-registers a one-shot
//! ephemeral runner, picks up the queued job, runs it, then self-deregisters.
//!
//! ## Why this is the mirror of [`crate::envelope_inject`]
//!
//! Both modules shape `ContainerSpec::env` at provision time. The injected
//! entries are ADDITIVE and cloud-provision-path only: `ContainerSpec::env` is
//! consumed solely by the Northflank `runtimeEnvironment` path; the hermetic
//! Docker path and `NoBoxProvisioner` ignore env, so the default backend
//! behaviour is unchanged.
//!
//! ## One env var, by construction
//!
//! The JIT config already encodes the runner name, the registration scope
//! (repo/org), the labels, and a one-time-use registration secret — all baked
//! in SERVER-SIDE by the broker via GitHub's `generate-jitconfig` (see
//! [`crate::runner_broker`]). So the box needs exactly this ONE env var; the
//! scope/labels never travel as separate plaintext box env.
//!
//! ## Secret hygiene
//!
//! The JIT config is a SENSITIVE, short-lived credential. It is read out exactly
//! here through the single grep-auditable seam [`JitRunnerConfig::expose`] and
//! pushed into the box env; it is NEVER logged. The runner-mode lease runs
//! UNTRUSTED customer CI (contract §4) with egress (ADR-0007 C2), but the
//! config is one-time-use and self-deregistering, so its blast radius is one
//! ephemeral runner registration.

use corelink_runner::lease::ContainerSpec;

use crate::runner_broker::JitRunnerConfig;

/// Env var the runner entrypoint reads (`deploy/runner/entrypoint.sh`): the
/// opaque JIT registration config consumed by `./run.sh --jitconfig`. The name
/// is part of the runner-image contract (ADR-0007 C3) — keep it in lock-step
/// with `deploy/runner/entrypoint.sh`.
pub const RUNNER_JITCONFIG_ENV: &str = "CORELINK_RUNNER_JITCONFIG";

/// Inject the JIT registration config into `spec.env` for a runner-mode box.
///
/// Additive: appends exactly one entry, preserving any env already present
/// (e.g. nothing for a runner lease today — the §13.2 ingest vars are
/// deliberately NOT injected on the runner path). The config is exposed here
/// (the single read seam) and never logged.
pub fn inject_runner_jitconfig(spec: &mut ContainerSpec, jitconfig: &JitRunnerConfig) {
    spec.env.push((
        RUNNER_JITCONFIG_ENV.to_string(),
        jitconfig.expose().to_string(),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_broker::{MockBroker, RunnerScope, RunnerTarget};

    fn bare_runner_spec() -> ContainerSpec {
        // A runner-mode spec as `ContainerSpec::from_runner_lease` builds it:
        // egress allowed, run-on-create, no env yet.
        ContainerSpec {
            name: "corelink-runner-x".to_string(),
            image: "ghcr.io/humangr-labs/corelink-runner@sha256:\
                    d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc"
                .to_string(),
            tmp_root: "/tmp/runner".to_string(),
            no_network: false,
            allow_egress: true,
            run_on_create: true,
            path_set: vec![],
            env: vec![],
        }
    }

    fn scope() -> RunnerScope {
        RunnerScope {
            target: RunnerTarget::Repo {
                owner: "humangr-labs".to_string(),
                repo: "corelink-runners".to_string(),
            },
            labels: vec!["corelink".to_string()],
        }
    }

    fn env_get<'a>(spec: &'a ContainerSpec, key: &str) -> Option<&'a str> {
        spec.env
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn injects_the_jitconfig_env_with_the_exposed_value() {
        let mut spec = bare_runner_spec();
        let cfg = JitRunnerConfig::new(MockBroker::derived_config(&scope()));
        let expected = cfg.expose().to_string();
        inject_runner_jitconfig(&mut spec, &cfg);
        assert_eq!(
            env_get(&spec, RUNNER_JITCONFIG_ENV),
            Some(expected.as_str())
        );
    }

    #[test]
    fn injection_is_additive_and_preserves_existing_env() {
        let mut spec = bare_runner_spec();
        spec.env
            .push(("PRE_EXISTING".to_string(), "keep-me".to_string()));
        let cfg = JitRunnerConfig::new("jit-cfg-bytes".to_string());
        inject_runner_jitconfig(&mut spec, &cfg);
        // The pre-existing entry survives, and exactly one entry was added.
        assert_eq!(env_get(&spec, "PRE_EXISTING"), Some("keep-me"));
        assert_eq!(env_get(&spec, RUNNER_JITCONFIG_ENV), Some("jit-cfg-bytes"));
        assert_eq!(spec.env.len(), 2);
    }

    #[test]
    fn the_env_var_name_matches_the_runner_image_contract() {
        // Lock-step with deploy/runner/entrypoint.sh — a rename here without a
        // matching image change would silently fail every runner registration.
        assert_eq!(RUNNER_JITCONFIG_ENV, "CORELINK_RUNNER_JITCONFIG");
    }
}
