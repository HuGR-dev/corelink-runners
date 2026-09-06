use super::*;
use crate::http::HttpRequest;

struct ExplodingTransport;

impl HttpTransport for ExplodingTransport {
    fn send(&self, _req: &HttpRequest) -> anyhow::Result<HttpResponse> {
        panic!("the Worker must not be contacted");
    }
}

fn config(spawn: &str, exec: &str, lifecycle: &str) -> CloudflareConfig {
    CloudflareConfig::new("https://spawn.example.dev", spawn).with_scoped_tokens(exec, lifecycle)
}

fn runner_spec() -> ContainerSpec {
    ContainerSpec {
        name: "auth-test".to_string(),
        image: "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc"
            .to_string(),
        tmp_root: "/tmp/job".to_string(),
        no_network: false,
        allow_egress: true,
        run_on_create: true,
        path_set: vec![],
        env: vec![],
    }
}

fn assert_no_transport_for_invalid_config(cfg: CloudflareConfig) {
    let engine = CloudflareEngine::new(ExplodingTransport, cfg);
    let c = RunningContainer {
        name: "h".to_string(),
    };
    assert!(engine.spawn(&runner_spec()).is_err());
    assert!(engine.exec_captured(&c, &["true"]).is_err());
    assert!(engine.is_alive(&c).is_err());
    assert!(engine.teardown(&c).is_err());
    assert!(engine.egress_cutoff(&c, false).is_err());
}

#[test]
fn from_env_rejects_whitespace_and_equal_scoped_tokens() {
    let env = |k: &str| match k {
        CLOUDFLARE_SPAWN_WORKER_URL_ENV => Some("https://spawn.example.dev".to_string()),
        CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV => Some("spawn".to_string()),
        CLOUDFLARE_EXEC_AUTH_TOKEN_ENV => Some("spawn".to_string()),
        CLOUDFLARE_LIFECYCLE_AUTH_TOKEN_ENV => Some("lifecycle".to_string()),
        _ => None,
    };
    assert!(CloudflareConfig::from_env_with(env).is_none());

    let whitespace = |k: &str| match k {
        CLOUDFLARE_SPAWN_WORKER_URL_ENV => Some("https://spawn.example.dev".to_string()),
        CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV => Some(" spawn".to_string()),
        CLOUDFLARE_EXEC_AUTH_TOKEN_ENV => Some("exec".to_string()),
        CLOUDFLARE_LIFECYCLE_AUTH_TOKEN_ENV => Some("lifecycle".to_string()),
        _ => None,
    };
    assert!(CloudflareConfig::from_env_with(whitespace).is_none());
}

#[test]
fn legacy_new_rejects_exec_without_transport_contact() {
    let engine = CloudflareEngine::new(
        ExplodingTransport,
        CloudflareConfig::new("https://spawn.example.dev", "spawn-only"),
    );
    let err = engine
        .exec_captured(
            &RunningContainer {
                name: "h".to_string(),
            },
            &["true"],
        )
        .expect_err("legacy config must reject unconfigured exec credentials");
    assert!(format!("{err:#}").contains(CLOUDFLARE_EXEC_AUTH_TOKEN_ENV));
}

#[test]
fn every_missing_token_fails_all_operations_before_transport() {
    for cfg in [
        config("", "exec", "lifecycle"),
        config("spawn", "", "lifecycle"),
        config("spawn", "exec", ""),
    ] {
        assert_no_transport_for_invalid_config(cfg);
    }
}

#[test]
fn every_pair_duplicate_fails_all_operations_before_transport() {
    for cfg in [
        config("same", "same", "lifecycle"),
        config("spawn", "same", "same"),
        config("same", "exec", "same"),
    ] {
        assert_no_transport_for_invalid_config(cfg);
    }
}
