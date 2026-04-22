use std::path::PathBuf;

use clap::Parser;

/// Runtime configuration for the Capsule daemon.
///
/// Values resolve in this order: CLI flag > environment variable > compiled default.
/// `vault_path` has no default — it must be supplied.
#[derive(Debug, Clone, Parser)]
#[command(name = "capsuled", version, about = "Capsule Node companion daemon")]
pub struct Config {
    /// Absolute path to the Obsidian vault this node serves.
    #[arg(long = "vault", env = "CAPSULE_VAULT_PATH")]
    pub vault_path: PathBuf,

    /// Localhost-only management API port (plugin ↔ daemon).
    #[arg(long, env = "CAPSULE_DAEMON_PORT", default_value_t = 7402)]
    pub daemon_port: u16,

    /// Public agent-facing capsule endpoint port.
    #[arg(long, env = "CAPSULE_EXTERNAL_PORT", default_value_t = 8402)]
    pub external_port: u16,

    /// Log filter (overrides RUST_LOG if set).
    #[arg(long, env = "CAPSULE_LOG_LEVEL", default_value = "info")]
    pub log_level: String,

    /// Seconds of inactivity after which the keyring auto-locks. `0`
    /// disables auto-lock entirely. Default 1800 (30 min) matches spec
    /// §9.4. "Activity" means an endpoint that consumed the unlocked
    /// master secret to produce its response.
    #[arg(
        long = "auto-lock-secs",
        env = "CAPSULE_KEYRING_AUTO_LOCK_SECS",
        default_value_t = 1800
    )]
    pub auto_lock_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_overrides_env_default() {
        let config =
            Config::try_parse_from(["capsuled", "--vault", "/tmp/vault", "--daemon-port", "9999"])
                .unwrap();
        assert_eq!(config.daemon_port, 9999);
        assert_eq!(config.external_port, 8402);
        assert_eq!(config.vault_path, PathBuf::from("/tmp/vault"));
    }

    #[test]
    fn defaults_match_spec() {
        let config = Config::try_parse_from(["capsuled", "--vault", "/tmp/vault"]).unwrap();
        assert_eq!(config.daemon_port, 7402);
        assert_eq!(config.external_port, 8402);
        assert_eq!(config.log_level, "info");
    }

    #[test]
    fn vault_path_is_required() {
        let result = Config::try_parse_from(["capsuled"]);
        assert!(result.is_err());
    }
}
