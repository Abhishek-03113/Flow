//! IPC connection authentication.
//!
//! The local IPC WebSocket (`flow_core::ipc::IPC_PORT`, bound to
//! `127.0.0.1`) previously accepted any client with no proof of identity
//! at all — `docs/contracts/README.md`'s "reachable only by the local
//! user" assumption was never actually enforced, just assumed. In
//! practice, `127.0.0.1` is reachable by *any* local process, including a
//! browser tab's own `WebSocket` — which cannot set arbitrary HTTP
//! headers, but *can* set the WebSocket subprotocol list (the second
//! argument to the JS `WebSocket` constructor, which becomes
//! `Sec-WebSocket-Protocol`). That's exactly why this uses that header
//! for the token rather than a normal one: it doesn't need a
//! browser-inaccessible transport feature to be secure, because the
//! actual secret — this randomly generated token, read from a file no
//! web page can access — is what a malicious page can never obtain,
//! regardless of which header carries it.
//!
//! A file (not a database row) so a completely independent process
//! (Flutter, a `websocat`/manual test client) can read it without
//! depending on `flow-core`'s SQLite schema.

use std::fs;
use std::io;
use std::path::PathBuf;

use rand::Rng;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Where the token lives: `~/.flow/ipc.token`. Deliberately *not*
/// `directories::ProjectDirs`'s per-OS nested app-data path — a plain
/// `<home>/.flow/...` path is trivial for an independent process in a
/// different language (Flutter's `ipc_daemon_repository.dart`) to
/// compute identically, without reimplementing `ProjectDirs`' per-OS
/// layout rules just to find one file.
pub fn token_path() -> PathBuf {
    let home = directories::BaseDirs::new()
        .expect("could not determine the current user's home directory")
        .home_dir()
        .to_path_buf();
    home.join(".flow").join("ipc.token")
}

/// Loads the persisted token, generating and persisting a new
/// cryptographically random one on first run. Every later call (this
/// process or a later restart) returns the identical token until the
/// file is deleted.
pub fn load_or_generate_token() -> String {
    let path = token_path();
    if let Ok(existing) = fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let token = generate_token();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("failed to create ~/.flow");
    }
    write_token_file(&path, &token).expect("failed to persist the IPC auth token");
    token
}

fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    crate::hex_encode(&bytes)
}

/// Writes the token file readable/writable only by its owner on Unix
/// (`0600`) — the whole point is that no other local user/process can
/// read it. Windows' per-user home directory ACLs already restrict this
/// without an explicit chmod-equivalent call here; tightening that
/// further is a real gap (`docs/release/bundling-and-release.md`'s
/// "Open decisions" territory), not something worth a Windows-specific
/// ACL call for a token whose actual secrecy already depends on desktop
/// account isolation either way.
fn write_token_file(path: &PathBuf, token: &str) -> io::Result<()> {
    fs::write(path, token)?;
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Points `HOME` (and `USERPROFILE`, for a Windows-hosted CI run
    /// executing this same test file) at a fresh temp dir for the
    /// duration of one test — `directories::BaseDirs` reads the home
    /// directory from the environment, not a mockable parameter, so
    /// isolating tests from a developer's real `~/.flow` means
    /// overriding it here rather than accepting `token_path()`'s
    /// default. Serialized via a shared mutex since env vars are
    /// process-global and `cargo test` runs this file's tests
    /// concurrently by default.
    fn with_isolated_home<T>(f: impl FnOnce() -> T) -> T {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let dir = tempfile_dir();
        let previous_home = std::env::var_os("HOME");
        let previous_userprofile = std::env::var_os("USERPROFILE");
        std::env::set_var("HOME", &dir);
        std::env::set_var("USERPROFILE", &dir);

        let result = f();

        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match previous_userprofile {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
        let _ = fs::remove_dir_all(&dir);
        result
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "flow-ipc-auth-test-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        fs::create_dir_all(&dir).expect("create temp home dir");
        dir
    }

    fn rand_suffix() -> u64 {
        let mut bytes = [0u8; 8];
        rand::rng().fill_bytes(&mut bytes);
        u64::from_le_bytes(bytes)
    }

    #[test]
    fn first_call_generates_a_token_and_persists_it() {
        with_isolated_home(|| {
            let token = load_or_generate_token();
            assert_eq!(token.len(), 64, "32 bytes hex-encoded is 64 characters");
            assert!(token_path().exists());
        });
    }

    #[test]
    fn a_second_call_returns_the_identical_token() {
        with_isolated_home(|| {
            let first = load_or_generate_token();
            let second = load_or_generate_token();
            assert_eq!(first, second);
        });
    }

    #[cfg(unix)]
    #[test]
    fn the_token_file_is_only_readable_by_its_owner() {
        with_isolated_home(|| {
            load_or_generate_token();
            let mode = fs::metadata(token_path())
                .expect("token file exists")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        });
    }
}
