//! Opt-in developer / testing switches, all read once at startup from the
//! environment. Every field is inert (`false` / [`SecurityMode::Secure`])
//! when its variable is unset, so a production `flow-daemon` that sets
//! none of these behaves exactly as it did before this module existed.
//!
//! Parsing is a pure function ([`DevMode::parse`]) with no logger and no
//! `std::env` access of its own, so it is unit-tested directly without
//! the process-global env lock the rest of `main.rs` needs.

/// Which security-seam implementations the daemon wires up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityMode {
    /// Real Noise encryption + ed25519 identity proof + trust-store gate
    /// + user pairing consent. The only mode a production daemon runs.
    Secure,
    /// Encryption, trust checks and pairing consent all replaced with
    /// permissive dev stand-ins, so two headless daemons can pair and
    /// stream with no UI attached. Requires `FLOW_DEV=1` as a guard.
    Insecure,
}

/// The resolved set of dev switches for this process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevMode {
    /// `FLOW_TRACE` — verbose `TRACE`-level structured logging.
    pub trace: bool,
    /// `FLOW_TEST_HOOKS` — registers the `debug_inject_input` IPC command.
    pub test_hooks: bool,
    /// `FLOW_SECURITY` (guarded by `FLOW_DEV`).
    pub security: SecurityMode,
    /// Startup warnings for `main` to log once the tracing subscriber is
    /// installed — this module's parsing is pure and has no logger.
    pub warnings: Vec<String>,
}

impl DevMode {
    /// Reads every switch from the real environment. Called once in
    /// `main`, before anything else.
    pub fn from_env() -> Self {
        Self::parse(
            std::env::var("FLOW_TRACE").ok().as_deref(),
            std::env::var("FLOW_TEST_HOOKS").ok().as_deref(),
            std::env::var("FLOW_SECURITY").ok().as_deref(),
            std::env::var("FLOW_DEV").ok().as_deref(),
        )
    }

    /// The pure core of [`Self::from_env`]: given the four raw variable
    /// values (each `None` when unset), resolve the switches and collect
    /// any warnings about misconfiguration.
    fn parse(
        trace: Option<&str>,
        test_hooks: Option<&str>,
        security: Option<&str>,
        dev: Option<&str>,
    ) -> Self {
        let mut warnings = Vec::new();
        let dev = is_truthy(dev);

        let security = match security.map(str::trim) {
            None | Some("") | Some("secure") => SecurityMode::Secure,
            Some("insecure") if dev => SecurityMode::Insecure,
            Some("insecure") => {
                warnings.push(
                    "FLOW_SECURITY=insecure ignored: FLOW_DEV=1 must be set as well. \
                     Staying on the secure path."
                        .to_string(),
                );
                SecurityMode::Secure
            }
            Some(other) => {
                warnings.push(format!(
                    "FLOW_SECURITY={other:?} is not a recognised value (expected \
                     'secure' or 'insecure'); staying on the secure path."
                ));
                SecurityMode::Secure
            }
        };

        Self {
            trace: is_truthy(trace),
            test_hooks: is_truthy(test_hooks),
            security,
            warnings,
        }
    }

    /// A loud multi-line banner for `main` to log whenever the daemon is
    /// *not* on the secure path, so an insecure daemon is never a quiet
    /// surprise in a log. `None` on the secure path.
    pub fn insecure_banner(&self) -> Option<&'static str> {
        match self.security {
            SecurityMode::Secure => None,
            SecurityMode::Insecure => Some(concat!(
                "============================================================\n",
                "flow-daemon RUNNING IN INSECURE DEV MODE (FLOW_SECURITY=insecure)\n",
                "peer traffic is UNENCRYPTED, incoming pairing is AUTO-ACCEPTED,\n",
                "and the trust store is BYPASSED. Never use this outside local dev.\n",
                "============================================================",
            )),
        }
    }
}

/// Whether an env value should count as "on". Deliberately narrow: only
/// the obvious affirmatives, so `FLOW_TRACE=0` / `false` / unset all
/// stay off.
fn is_truthy(value: Option<&str>) -> bool {
    matches!(
        value.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_unset_is_fully_inert() {
        let mode = DevMode::parse(None, None, None, None);
        assert_eq!(
            mode,
            DevMode {
                trace: false,
                test_hooks: false,
                security: SecurityMode::Secure,
                warnings: Vec::new(),
            }
        );
        assert_eq!(mode.insecure_banner(), None);
    }

    #[test]
    fn trace_and_test_hooks_accept_common_affirmatives_only() {
        assert!(DevMode::parse(Some("1"), None, None, None).trace);
        assert!(DevMode::parse(Some("true"), None, None, None).trace);
        assert!(DevMode::parse(Some(" ON "), None, None, None).trace);
        assert!(!DevMode::parse(Some("0"), None, None, None).trace);
        assert!(!DevMode::parse(Some("false"), None, None, None).trace);

        assert!(DevMode::parse(None, Some("yes"), None, None).test_hooks);
        assert!(!DevMode::parse(None, Some(""), None, None).test_hooks);
    }

    #[test]
    fn insecure_security_needs_the_flow_dev_guard() {
        let guarded = DevMode::parse(None, None, Some("insecure"), Some("1"));
        assert_eq!(guarded.security, SecurityMode::Insecure);
        assert!(guarded.warnings.is_empty());
        assert!(guarded.insecure_banner().is_some());
    }

    #[test]
    fn insecure_without_the_guard_falls_back_to_secure_with_a_warning() {
        let unguarded = DevMode::parse(None, None, Some("insecure"), None);
        assert_eq!(unguarded.security, SecurityMode::Secure);
        assert_eq!(unguarded.warnings.len(), 1);
        assert!(unguarded.warnings[0].contains("FLOW_DEV=1"));
    }

    #[test]
    fn an_unrecognised_security_value_warns_and_stays_secure() {
        let bogus = DevMode::parse(None, None, Some("off"), Some("1"));
        assert_eq!(bogus.security, SecurityMode::Secure);
        assert_eq!(bogus.warnings.len(), 1);
        assert!(bogus.warnings[0].contains("not a recognised value"));
    }

    #[test]
    fn explicit_secure_is_the_same_as_unset() {
        assert_eq!(
            DevMode::parse(None, None, Some("secure"), None),
            DevMode::parse(None, None, None, None)
        );
    }
}
