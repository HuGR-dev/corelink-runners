//! AU1.9 — production exec-server auth comes only from a mode-0400 file.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use corelink_check_exec_server::{
    ALLOW_UNAUTH_ENV, AUTH_TOKEN_ENV, AUTH_TOKEN_FILE_ENV, ExecAuth, ExecAuthError, app,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "corelink-check-exec-auth-file-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create scratch dir");
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_secret(path: &Path, value: &str, mode: u32) {
    fs::write(path, value).expect("write secret fixture");
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("chmod secret fixture");
}

fn clear_auth_env() {
    // SAFETY: this integration test is deliberately a single test case, so its
    // process has no concurrent test mutating these variables.
    unsafe {
        std::env::remove_var(AUTH_TOKEN_ENV);
        std::env::remove_var(AUTH_TOKEN_FILE_ENV);
        std::env::remove_var(ALLOW_UNAUTH_ENV);
    }
}

#[test]
fn production_auth_accepts_only_a_0400_secret_file() {
    clear_auth_env();
    let scratch = Scratch::new();
    let secret = scratch.path("exec-token");
    write_secret(&secret, "file-only-token", 0o400);

    // SAFETY: see clear_auth_env; this test owns its integration-test process.
    unsafe { std::env::set_var(AUTH_TOKEN_FILE_ENV, &secret) };
    let auth = ExecAuth::from_secret_file_env().expect("0400 regular file is accepted");
    assert!(auth.is_authenticated());
    assert!(
        app().is_ok(),
        "the production router uses the file resolver"
    );

    fs::set_permissions(&secret, fs::Permissions::from_mode(0o640)).expect("chmod fixture");
    assert!(matches!(
        ExecAuth::from_secret_file_env(),
        Err(ExecAuthError::TokenFileMode { mode: 0o640, .. })
    ));

    write_secret(&secret, "", 0o400);
    assert_eq!(
        ExecAuth::from_secret_file_env().unwrap_err(),
        ExecAuthError::EmptyToken
    );

    fs::remove_file(&secret).expect("remove fixture");
    assert!(matches!(
        ExecAuth::from_secret_file_env(),
        Err(ExecAuthError::TokenFileMissing { .. })
    ));

    // A symlink with 0400 target permissions is still not a secret mount: the
    // resolver opens with O_NOFOLLOW and refuses it.
    let target = scratch.path("target");
    let link = scratch.path("link");
    write_secret(&target, "symlink-token", 0o400);
    std::os::unix::fs::symlink(&target, &link).expect("create symlink fixture");
    unsafe { std::env::set_var(AUTH_TOKEN_FILE_ENV, &link) };
    assert!(matches!(
        ExecAuth::from_secret_file_env(),
        Err(ExecAuthError::TokenFileOpen { .. })
    ));

    // The legacy value is rejected even when a valid file exists: otherwise
    // the server would authenticate correctly while still leaking the same
    // credential through /proc/*/environ.
    unsafe {
        std::env::set_var(AUTH_TOKEN_FILE_ENV, &target);
        std::env::set_var(AUTH_TOKEN_ENV, "legacy-env-secret");
    }
    assert_eq!(
        ExecAuth::from_secret_file_env().unwrap_err(),
        ExecAuthError::LegacyTokenEnvironment
    );

    // Exercise the real binary's boot boundary, not only the library method.
    // With only the old env it must exit before bind/listen and must not echo the
    // value in its diagnostic.
    let output = Command::new(env!("CARGO_BIN_EXE_corelink-check-exec-server"))
        .env(AUTH_TOKEN_ENV, "legacy-value-must-not-be-echoed")
        .env_remove(AUTH_TOKEN_FILE_ENV)
        .env_remove(ALLOW_UNAUTH_ENV)
        .output()
        .expect("run production binary");
    assert!(!output.status.success(), "env-only boot must be refused");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(AUTH_TOKEN_FILE_ENV),
        "refusal should direct the operator to the file surface: {stderr}"
    );
    assert!(
        !stderr.contains("legacy-value-must-not-be-echoed"),
        "refusal diagnostics must not echo the rejected secret"
    );

    clear_auth_env();
}
