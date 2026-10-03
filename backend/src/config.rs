//! Configuration management.

use crate::paths::{DataPaths, PathConfig};
use figment::{
    providers::{Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration structure that matches the TOML file format.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    server: ServerConfig,
    #[serde(default)]
    storage: StorageConfig,
    #[serde(default)]
    logging: LoggingConfig,
    #[serde(default)]
    discovery: DiscoveryConfig,
    #[serde(default)]
    nmos: NmosFile,
    #[serde(default)]
    ports: PortsConfig,
}

/// `[ports]` section: the port numbers Strom administers and hands out.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PortsConfig {
    /// Each entry is a range (`"47100-47199"`) or a single port (`47250`), in
    /// either case expanded into the pool. Empty switches the pool off: a
    /// Strom nobody shares has no ports to administer, and a pool picked for
    /// it by default would only narrow which ports its own flows may bind.
    #[serde(default)]
    ports: Vec<PortConfigEntry>,
    /// Lifetime a reservation gets when the caller does not say.
    lease_ttl_seconds: Option<u64>,
    /// Whether to bind-probe a candidate port before handing it out.
    probe_before_handout: Option<bool>,
}

/// One entry in the configured port pool. TOML keeps quoted ranges as strings
/// and bare single ports as integers, so both forms must deserialize.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum PortConfigEntry {
    Port(u16),
    Spec(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ServerConfig {
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default = "default_ice_servers")]
    ice_servers: Vec<String>,
    #[serde(default = "default_ice_transport_policy")]
    ice_transport_policy: String,
    /// CORS allowed origins. If empty, allows any origin.
    #[serde(default)]
    cors_allowed_origins: Vec<String>,
    /// Path to TLS certificate file (PEM format).
    #[serde(default)]
    tls_cert: Option<PathBuf>,
    /// Path to TLS private key file (PEM format).
    #[serde(default)]
    tls_key: Option<PathBuf>,
}

fn default_ice_servers() -> Vec<String> {
    vec!["stun:stun.l.google.com:19302".to_string()]
}

fn default_ice_transport_policy() -> String {
    "all".to_string()
}

/// Normalize an ICE server URL to RFC 7064/7065 format.
/// Converts GStreamer-style URLs (stun://, turn://) to standard format (stun:, turn:).
fn normalize_ice_server_url(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("stun://") {
        format!("stun:{}", rest)
    } else if let Some(rest) = url.strip_prefix("turn://") {
        format!("turn:{}", rest)
    } else if let Some(rest) = url.strip_prefix("turns://") {
        format!("turns:{}", rest)
    } else {
        url.to_string()
    }
}

/// Normalize a list of ICE server URLs to RFC format.
fn normalize_ice_servers(servers: Vec<String>) -> Vec<String> {
    servers
        .into_iter()
        .map(|s| normalize_ice_server_url(&s))
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StorageConfig {
    database_url: Option<String>,
    data_dir: Option<PathBuf>,
    flows_path: Option<PathBuf>,
    blocks_path: Option<PathBuf>,
    media_path: Option<PathBuf>,
    cef_cache_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LoggingConfig {
    /// Path to log file (if set, logs will be written to file in addition to stdout)
    log_file: Option<PathBuf>,
    /// Log level (trace, debug, info, warn, error)
    /// If not set, uses RUST_LOG environment variable or defaults to "info"
    log_level: Option<String>,
    /// Stdout log format ("compact" or "json"). Raw string so blank values fall back to the
    /// default instead of a deserialize error (same pattern as `log_file` / `log_level`).
    stdout_log_format: Option<String>,
    /// Emit selected StromEvent lifecycle/error events as structured tracing logs.
    #[serde(default)]
    structured_events: bool,
    /// Include high-frequency events (meters, stats, ...) in structured event logs.
    #[serde(default)]
    include_high_frequency_events: bool,
}

/// Stdout log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable, single-line-per-event output (current default behavior).
    #[default]
    Compact,
    /// Structured JSON output, one object per line, suitable for log collectors.
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscoveryConfig {
    /// SAP multicast addresses to listen on and announce to.
    /// Default: ["239.255.255.255", "224.2.127.254"] (AES67 + global scope)
    #[serde(default = "default_sap_multicast_addresses")]
    sap_multicast_addresses: Vec<String>,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            sap_multicast_addresses: default_sap_multicast_addresses(),
        }
    }
}

fn default_sap_multicast_addresses() -> Vec<String> {
    vec![
        "239.255.255.255".to_string(), // AES67/Dante (admin-scoped)
        "224.2.127.254".to_string(),   // Global scope (broadcast)
    ]
}

/// Environment variables that map to a scalar config key.
///
/// These have to be listed explicitly. A generic `Env::prefixed("STROM_")`
/// provider splits the variable name on every underscore, which only produces
/// the right key for single-word fields: `STROM_SERVER_ICE_TRANSPORT_POLICY`
/// becomes `server.ice.transport.policy`, which no field matches, so the
/// variable is silently ignored.
const SCALAR_ENV_VARS: &[(&str, &str)] = &[
    (
        "STROM_SERVER_ICE_TRANSPORT_POLICY",
        "server.ice_transport_policy",
    ),
    ("STROM_TLS_CERT", "server.tls_cert"),
    ("STROM_TLS_KEY", "server.tls_key"),
    ("STROM_STORAGE_DATABASE_URL", "storage.database_url"),
    ("STROM_STORAGE_DATA_DIR", "storage.data_dir"),
    ("STROM_STORAGE_FLOWS_PATH", "storage.flows_path"),
    ("STROM_STORAGE_BLOCKS_PATH", "storage.blocks_path"),
    ("STROM_STORAGE_MEDIA_PATH", "storage.media_path"),
    ("STROM_STORAGE_CEF_CACHE_PATH", "storage.cef_cache_path"),
    ("STROM_LOGGING_LOG_FILE", "logging.log_file"),
    ("STROM_LOGGING_LOG_LEVEL", "logging.log_level"),
    ("STROM_NMOS_REGISTRY", "nmos.registry"),
    ("STROM_NMOS_HOST", "nmos.host"),
    ("STROM_NMOS_LABEL", "nmos.label"),
];

/// Environment variables that map to a list config key, comma-separated.
const LIST_ENV_VARS: &[(&str, &str)] = &[
    ("STROM_SERVER_ICE_SERVERS", "server.ice_servers"),
    (
        "STROM_SERVER_CORS_ALLOWED_ORIGINS",
        "server.cors_allowed_origins",
    ),
    (
        "STROM_DISCOVERY_SAP_MULTICAST_ADDRESSES",
        "discovery.sap_multicast_addresses",
    ),
];

/// Splits a comma-separated environment variable into non-empty entries.
fn split_list_env(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn default_port() -> u16 {
    strom_types::DEFAULT_PORT
}

/// `[nmos]` table. Domain paths default to `/dev/shm/mxl` when the key is omitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NmosFile {
    #[serde(default = "default_nmos_enabled")]
    enabled: bool,
    #[serde(default)]
    registry: Option<String>,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    label: Option<String>,
    /// `None` means the key was omitted and the built-in default path is used.
    #[serde(default)]
    domains: Option<Vec<String>>,
}

impl Default for NmosFile {
    fn default() -> Self {
        Self {
            enabled: default_nmos_enabled(),
            registry: None,
            host: None,
            label: None,
            domains: None,
        }
    }
}

fn default_nmos_enabled() -> bool {
    true
}

/// Application configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Port to listen on
    pub port: u16,
    /// Path to flows storage file (used if database_url is None)
    pub flows_path: PathBuf,
    /// Path to blocks storage file
    pub blocks_path: PathBuf,
    /// Path to media files directory
    pub media_path: PathBuf,
    /// Directory holding the CEF/Chromium profile used by `cefsrc`
    pub cef_cache_path: PathBuf,
    /// PostgreSQL database URL (if set, PostgreSQL is used instead of JSON files)
    /// Format: postgresql://user:password@host/database_name
    pub database_url: Option<String>,
    /// Path to log file (if set, logs will be written to file in addition to stdout)
    pub log_file: Option<PathBuf>,
    /// Log level (if set, overrides RUST_LOG environment variable)
    pub log_level: Option<String>,
    /// Stdout log format: compact (human-readable) or JSON (structured)
    pub stdout_log_format: LogFormat,
    /// Emit selected StromEvent lifecycle/error events as structured tracing logs
    pub structured_events: bool,
    /// Include high-frequency events (meters, stats, ...) in structured event logs
    pub include_high_frequency_events: bool,
    /// ICE servers for WebRTC NAT traversal (STUN/TURN)
    /// Format: stun:host:port or turn:user:pass@host:port
    pub ice_servers: Vec<String>,
    /// ICE transport policy for WebRTC connections
    /// "all" (default) = use all candidate types (host, srflx, relay)
    /// "relay" = only use TURN relay candidates
    pub ice_transport_policy: String,
    /// SAP multicast addresses to listen on and announce to.
    /// Default: ["239.255.255.255", "224.2.127.254"] (AES67 + global scope)
    pub sap_multicast_addresses: Vec<String>,
    /// CORS allowed origins. If empty, allows any origin.
    pub cors_allowed_origins: Vec<String>,
    /// Path to TLS certificate file (PEM format). Enables HTTPS when paired with tls_key.
    pub tls_cert: Option<PathBuf>,
    /// Path to TLS private key file (PEM format). Enables HTTPS when paired with tls_cert.
    pub tls_key: Option<PathBuf>,
    /// Announce and register an NMOS node for MXL flows.
    pub nmos_enabled: bool,
    /// IS-04 registry base URL, for example `http://192.0.2.10:3210`.
    pub nmos_registry: Option<String>,
    /// Host advertised in the Node API. Empty selects a non-loopback IPv4 address.
    pub nmos_host: Option<String>,
    /// Node label. Defaults to `Strom`.
    pub nmos_label: String,
    /// MXL domain directories that contain `domain_def.json`.
    pub nmos_domains: Vec<PathBuf>,
    /// `NMOS_SEED`. When set, NMOS ids are UUIDv5 of this value.
    pub nmos_seed: Option<String>,
    /// `NMOS_TAGS` JSON object of tag name to string arrays.
    pub nmos_tags: std::collections::HashMap<String, Vec<String>>,
    /// `NMOS_DNS_SD`. Default false: no registry browse and no node advertisement.
    pub nmos_dns_sd: bool,
    /// `NMOS_QUERY_ADDRESS`. Defaults to the registration address.
    pub nmos_query_address: Option<String>,
    /// `NMOS_QUERY_PORT`. Defaults to the registration port plus one.
    pub nmos_query_port: Option<u16>,
    /// Parent of MXL domain directories (`MXL_DOMAIN_SCAN_PATH`).
    pub mxl_scan_path: PathBuf,
    /// This function's output domain directory (`MXL_OUTPUT_DOMAIN_DIR`).
    pub mxl_output_domain_dir: Option<PathBuf>,
    /// `MXL_OUTPUT_DOMAIN_ID`, or the id derived from `NMOS_SEED`.
    pub mxl_output_domain_id: Option<uuid::Uuid>,
    /// Nanoseconds written into a new domain `options.json`.
    pub mxl_history_duration_ns: u64,
    /// `MXL_CLEANUP_ON_EXIT`.
    pub mxl_cleanup_on_exit: bool,
    /// `SHUTDOWN_TIMEOUT_S`.
    pub shutdown_timeout_s: u64,
    /// Port numbers the pool may hand out. Empty switches the pool off.
    pub pool_ports: std::collections::BTreeSet<u16>,
    /// Lifetime a reservation gets when the caller does not say.
    pub port_lease_ttl_seconds: u64,
    /// Whether to bind-probe a candidate port before handing it out.
    pub probe_before_handout: bool,
}

/// Expand the `[ports] ports` entries into the pool.
///
/// A range is notation: `"47100-47199"` and `47250` both land in the same set,
/// which is what lets an operator punch a hole in a range by listing the
/// pieces around it. An empty list means no pool at all — never a silent
/// default range.
fn pool_ports(entries: &[PortConfigEntry]) -> anyhow::Result<std::collections::BTreeSet<u16>> {
    let mut ports = std::collections::BTreeSet::new();
    for entry in entries {
        let spec = match entry {
            PortConfigEntry::Port(port) => port.to_string(),
            PortConfigEntry::Spec(spec) => spec.trim().to_string(),
        };
        if spec.is_empty() {
            continue;
        }
        let expanded = strom_types::ports::parse_port_spec(&spec)
            .map_err(|e| anyhow::anyhow!("invalid ports.ports entry '{spec}': {e}"))?;
        ports.extend(expanded);
    }
    Ok(ports)
}

fn port_lease_ttl(configured: Option<u64>) -> anyhow::Result<u64> {
    let ttl = configured.unwrap_or(strom_types::ports::DEFAULT_PORT_LEASE_TTL_SECS);
    if ttl == 0 || ttl > strom_types::ports::MAX_PORT_LEASE_TTL_SECS {
        anyhow::bail!(
            "ports.lease_ttl_seconds / STROM_PORT_LEASE_TTL must be between 1 and {}, got {ttl}",
            strom_types::ports::MAX_PORT_LEASE_TTL_SECS
        );
    }
    Ok(ttl)
}

/// A blank path is not a path. Blank environment variables are gone before
/// this runs (see `strom_types::env`), but a config file can carry
/// `tls_cert = ""` just as easily.
fn non_blank_path(path: Option<PathBuf>) -> Option<PathBuf> {
    path.filter(|p| !p.to_string_lossy().trim().is_empty())
}

/// Parse the raw `logging.stdout_log_format` config value. A blank or absent value falls back to
/// the default (`compact`) rather than a deserialize error — matching the blank-value
/// handling already applied to `log_file` and `log_level`. Anything other than `"compact"`
/// or `"json"` (case-insensitive) is rejected with a clear error instead of silently
/// defaulting, so a typo doesn't go unnoticed.
fn parse_stdout_log_format(value: Option<String>) -> anyhow::Result<LogFormat> {
    let normalized = strom_types::env::non_blank(value).map(|s| s.to_lowercase());
    match normalized.as_deref() {
        None => Ok(LogFormat::default()),
        Some("compact") => Ok(LogFormat::Compact),
        Some("json") => Ok(LogFormat::Json),
        Some(other) => {
            anyhow::bail!(
                "invalid logging.stdout_log_format '{other}': expected \"compact\" or \"json\""
            )
        }
    }
}

impl Config {
    /// Load configuration with full priority chain: CLI args > env vars > config files > defaults.
    ///
    /// This is the recommended way to load configuration, as it supports config files.
    /// Config files are searched in this order:
    /// 1. `.strom.toml` in current directory
    /// 2. `config.toml` in user config directory (~/.config/strom/ on Linux)
    #[allow(clippy::too_many_arguments)]
    pub fn from_figment(
        port: Option<u16>,
        data_dir: Option<PathBuf>,
        flows_path: Option<PathBuf>,
        blocks_path: Option<PathBuf>,
        media_path: Option<PathBuf>,
        cef_cache_path: Option<PathBuf>,
        database_url: Option<String>,
        tls_cert: Option<PathBuf>,
        tls_key: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        // Find config file paths
        let local_config = std::env::current_dir().ok().map(|d| d.join(".strom.toml"));
        let user_config = directories::ProjectDirs::from("", "", "strom")
            .map(|dirs| dirs.config_dir().join("config.toml"));

        // Build figment with priority: defaults < user config < local config < env vars < CLI args
        let mut figment = Figment::new();

        // 1. Start with defaults
        figment = figment.merge(Serialized::defaults(ConfigFile {
            server: ServerConfig {
                port: strom_types::DEFAULT_PORT,
                ice_servers: default_ice_servers(),
                ice_transport_policy: default_ice_transport_policy(),
                cors_allowed_origins: Vec::new(),
                tls_cert: None,
                tls_key: None,
            },
            storage: StorageConfig::default(),
            logging: LoggingConfig::default(),
            discovery: DiscoveryConfig::default(),
            nmos: NmosFile::default(),
            ports: PortsConfig::default(),
        }));

        // 2. Merge user config file if it exists
        if let Some(ref path) = user_config {
            if path.exists() {
                figment = figment.merge(Toml::file(path));
            }
        }

        // 3. Merge local config file if it exists
        if let Some(ref path) = local_config {
            if path.exists() {
                figment = figment.merge(Toml::file(path));
            }
        }

        // 4. Merge environment variables (STROM_* prefix).
        //
        // Every key is mapped explicitly. A generic provider that splits the
        // variable name on underscores cannot express a field name that itself
        // contains one, and figment does not parse comma-separated lists.
        if let Some(port) = agreed_listen_port()? {
            figment = figment.merge(Serialized::default("server.port", port));
        }
        for (var, key) in SCALAR_ENV_VARS {
            if let Some(value) = strom_types::env::var_opt(var) {
                figment = figment.merge(Serialized::default(key, value));
            }
        }
        for (var, key) in LIST_ENV_VARS {
            if let Some(raw) = strom_types::env::var_opt(var) {
                let values = split_list_env(&raw);
                if !values.is_empty() {
                    figment = figment.merge(Serialized::default(key, values));
                }
            }
        }

        // 4d. Handle STROM_LOGGING_STDOUT_LOG_FORMAT / STROM_LOGGING_STRUCTURED_EVENTS /
        // STROM_LOGGING_INCLUDE_HIGH_FREQUENCY_EVENTS specially, for the same
        // reason as 4c: underscores in the field names break the split("_")
        // env mapping above.
        if let Some(format) = strom_types::env::var_opt("STROM_LOGGING_STDOUT_LOG_FORMAT") {
            figment = figment.merge(Serialized::default(
                "logging.stdout_log_format",
                format.to_lowercase(),
            ));
        }
        if let Some(val) = strom_types::env::var_opt("STROM_LOGGING_STRUCTURED_EVENTS") {
            if let Ok(enabled) = val.parse::<bool>() {
                figment = figment.merge(Serialized::default("logging.structured_events", enabled));
            }
        }
        if let Some(val) = strom_types::env::var_opt("STROM_LOGGING_INCLUDE_HIGH_FREQUENCY_EVENTS")
        {
            if let Ok(enabled) = val.parse::<bool>() {
                figment = figment.merge(Serialized::default(
                    "logging.include_high_frequency_events",
                    enabled,
                ));
            }
        }

        // 4e. Port pool settings, each named explicitly for the same reason.
        if let Some(raw) = strom_types::env::var_opt("STROM_PORTS") {
            let entries = split_list_env(&raw);
            if !entries.is_empty() {
                figment = figment.merge(Serialized::default("ports.ports", entries));
            }
        }
        if let Some(ttl) = strom_types::env::var_opt("STROM_PORT_LEASE_TTL") {
            let ttl: u64 = ttl
                .parse()
                .map_err(|_| anyhow::anyhow!("STROM_PORT_LEASE_TTL is not a number: {ttl}"))?;
            figment = figment.merge(Serialized::default("ports.lease_ttl_seconds", ttl));
        }
        if let Some(probe) = strom_types::env::var_opt("STROM_PORT_PROBE_BEFORE_HANDOUT") {
            let probe = match probe.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                _ => anyhow::bail!(
                    "STROM_PORT_PROBE_BEFORE_HANDOUT must be true or false, got {probe}"
                ),
            };
            figment = figment.merge(Serialized::default("ports.probe_before_handout", probe));
        }
        if let Some(val) = strom_types::env::var_opt("STROM_NMOS_ENABLED") {
            let enabled = match val.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                _ => anyhow::bail!("STROM_NMOS_ENABLED must be true or false, got {val}"),
            };
            figment = figment.merge(Serialized::default("nmos.enabled", enabled));
        }
        if let Some(domains) = strom_types::env::var_opt("STROM_NMOS_DOMAINS") {
            let domains: Vec<String> = domains
                .split(',')
                .map(|entry| entry.trim().to_string())
                .filter(|entry| !entry.is_empty())
                .collect();
            figment = figment.merge(Serialized::default("nmos.domains", domains));
        }

        // CONFIG_DIR is the state directory. STROM_DATA_DIR is the older name.
        if let Some(dir) = strom_types::env::var_opt("STROM_DATA_DIR") {
            figment = figment.merge(Serialized::default("storage.data_dir", PathBuf::from(dir)));
        }
        if let Some(dir) = strom_types::env::var_opt("CONFIG_DIR") {
            figment = figment.merge(Serialized::default("storage.data_dir", PathBuf::from(dir)));
        }

        // 5. Merge CLI arguments (highest priority)
        if let Some(ref cert) = tls_cert {
            figment = figment.merge(Serialized::default("server.tls_cert", cert));
        }
        if let Some(ref key) = tls_key {
            figment = figment.merge(Serialized::default("server.tls_key", key));
        }
        if let Some(p) = port {
            figment = figment.merge(Serialized::default("server.port", p));
        }
        if let Some(ref dd) = data_dir {
            figment = figment.merge(Serialized::default("storage.data_dir", dd));
        }
        if let Some(ref fp) = flows_path {
            figment = figment.merge(Serialized::default("storage.flows_path", fp));
        }
        if let Some(ref bp) = blocks_path {
            figment = figment.merge(Serialized::default("storage.blocks_path", bp));
        }
        if let Some(ref mp) = media_path {
            figment = figment.merge(Serialized::default("storage.media_path", mp));
        }
        if let Some(ref cp) = cef_cache_path {
            figment = figment.merge(Serialized::default("storage.cef_cache_path", cp));
        }
        if let Some(ref db) = database_url {
            figment = figment.merge(Serialized::default("storage.database_url", db));
        }

        // Extract the configuration
        let config_file: ConfigFile = figment.extract()?;

        // Resolve data paths
        let path_config = PathConfig {
            data_dir: non_blank_path(config_file.storage.data_dir),
            flows_path: non_blank_path(config_file.storage.flows_path),
            blocks_path: non_blank_path(config_file.storage.blocks_path),
            media_path: non_blank_path(config_file.storage.media_path),
            cef_cache_path: non_blank_path(config_file.storage.cef_cache_path),
        };
        let data_paths = DataPaths::resolve(path_config)?;

        let mut built = Self {
            port: config_file.server.port,
            flows_path: data_paths.flows_path,
            blocks_path: data_paths.blocks_path,
            media_path: data_paths.media_path,
            cef_cache_path: data_paths.cef_cache_path,
            database_url: strom_types::env::non_blank(config_file.storage.database_url),
            log_file: non_blank_path(config_file.logging.log_file),
            log_level: strom_types::env::non_blank(config_file.logging.log_level),
            stdout_log_format: parse_stdout_log_format(config_file.logging.stdout_log_format)?,
            structured_events: config_file.logging.structured_events,
            include_high_frequency_events: config_file.logging.include_high_frequency_events,
            ice_servers: normalize_ice_servers(config_file.server.ice_servers),
            ice_transport_policy: config_file.server.ice_transport_policy,
            sap_multicast_addresses: config_file.discovery.sap_multicast_addresses,
            cors_allowed_origins: config_file.server.cors_allowed_origins,
            tls_cert: non_blank_path(config_file.server.tls_cert),
            tls_key: non_blank_path(config_file.server.tls_key),
            nmos_enabled: config_file.nmos.enabled,
            nmos_registry: config_file.nmos.registry.filter(|value| !value.is_empty()),
            nmos_host: config_file.nmos.host.filter(|value| !value.is_empty()),
            nmos_label: config_file
                .nmos
                .label
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| "Strom".to_string()),
            nmos_domains: config_file
                .nmos
                .domains
                .unwrap_or_else(|| vec![strom_types::mxl::DEFAULT_MXL_DOMAIN.to_string()])
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            nmos_seed: None,
            nmos_tags: std::collections::HashMap::new(),
            nmos_dns_sd: false,
            nmos_query_address: None,
            nmos_query_port: None,
            mxl_scan_path: PathBuf::from("/Volumes/mxl"),
            mxl_output_domain_dir: None,
            mxl_output_domain_id: None,
            mxl_history_duration_ns: crate::nmos::DEFAULT_HISTORY_DURATION_NS,
            mxl_cleanup_on_exit: false,
            shutdown_timeout_s: 10,
            pool_ports: pool_ports(&config_file.ports.ports)?,
            port_lease_ttl_seconds: port_lease_ttl(config_file.ports.lease_ttl_seconds)?,
            probe_before_handout: config_file.ports.probe_before_handout.unwrap_or(true),
        };
        built.apply_platform_env()?;
        Ok(built)
    }

    /// Environment variables that name the platform contract. File values and
    /// `STROM_*` aliases are already on `self`; these override them.
    fn apply_platform_env(&mut self) -> anyhow::Result<()> {
        if let Some(seed) = first_env(&["NMOS_SEED"])? {
            self.nmos_seed = Some(seed);
        }
        if let Some(label) = first_env(&["NMOS_LABEL", "STROM_NMOS_LABEL"])? {
            self.nmos_label = label;
        }
        if let Some(raw) = first_env(&["NMOS_TAGS"])? {
            self.nmos_tags = parse_tags(&raw)?;
        }
        if let Some(raw) = first_env(&["NMOS_DNS_SD"])? {
            self.nmos_dns_sd = parse_bool_env("NMOS_DNS_SD", &raw)?;
        }
        if let Some(address) = first_env(&["NMOS_QUERY_ADDRESS"])? {
            if address.parse::<std::net::Ipv4Addr>().is_err() {
                anyhow::bail!("NMOS_QUERY_ADDRESS must be an IPv4 address, got {address}");
            }
            self.nmos_query_address = Some(address);
        }
        if let Some(raw) = first_env(&["NMOS_QUERY_PORT"])? {
            self.nmos_query_port = Some(
                raw.parse()
                    .map_err(|_| anyhow::anyhow!("NMOS_QUERY_PORT is not a valid port: {raw}"))?,
            );
        }
        if let Some(raw) = first_env(&["NMOS_ENABLED", "STROM_NMOS_ENABLED"])? {
            self.nmos_enabled = parse_bool_env("NMOS_ENABLED", &raw)?;
        }
        let registry_address = first_env(&["NMOS_REGISTRY_ADDRESS"])?;
        let registry_port = first_env(&["NMOS_REGISTRY_PORT"])?;
        let legacy_registry = first_env(&["STROM_NMOS_REGISTRY"])?;
        self.nmos_registry = registry_url(
            registry_address.as_deref(),
            registry_port.as_deref(),
            legacy_registry.as_deref(),
            self.nmos_registry.as_deref(),
        )?;
        if let Some(path) = first_env(&["MXL_DOMAIN_SCAN_PATH"])? {
            self.mxl_scan_path = PathBuf::from(path);
        }
        if let Some(path) = first_env(&["MXL_OUTPUT_DOMAIN_DIR"])? {
            self.mxl_output_domain_dir = Some(PathBuf::from(path));
        }
        if let Some(raw) = first_env(&["MXL_OUTPUT_DOMAIN_ID"])? {
            self.mxl_output_domain_id = Some(
                uuid::Uuid::parse_str(raw.trim())
                    .map_err(|_| anyhow::anyhow!("MXL_OUTPUT_DOMAIN_ID is not a UUID: {raw}"))?,
            );
        } else if self.mxl_output_domain_id.is_none() {
            if let Some(seed) = &self.nmos_seed {
                self.mxl_output_domain_id = Some(crate::nmos::output_domain_id_from_seed(seed));
            }
        }
        if let Some(raw) = first_env(&["MXL_HISTORY_DURATION_NS"])? {
            self.mxl_history_duration_ns = raw.parse().map_err(|_| {
                anyhow::anyhow!(
                    "MXL_HISTORY_DURATION_NS is not an integer number of nanoseconds: {raw}"
                )
            })?;
        }
        if let Some(raw) = first_env(&["MXL_CLEANUP_ON_EXIT"])? {
            self.mxl_cleanup_on_exit = parse_bool_env("MXL_CLEANUP_ON_EXIT", &raw)?;
        }
        if let Some(raw) = first_env(&["SHUTDOWN_TIMEOUT_S"])? {
            let seconds: u64 = raw
                .parse()
                .map_err(|_| anyhow::anyhow!("SHUTDOWN_TIMEOUT_S is not a number: {raw}"))?;
            if seconds == 0 {
                anyhow::bail!("SHUTDOWN_TIMEOUT_S must be at least 1");
            }
            self.shutdown_timeout_s = seconds;
        }
        let host = first_env(&["NMOS_HOST_ADDRESS", "STROM_NMOS_HOST"])?;
        if let Some(host) = host {
            self.nmos_host = Some(
                crate::nmos::require_announce_ipv4(&host).map_err(|err| anyhow::anyhow!(err))?,
            );
        } else if self.nmos_enabled {
            if let Some(existing) = self.nmos_host.clone() {
                self.nmos_host = Some(
                    crate::nmos::require_announce_ipv4(&existing)
                        .map_err(|err| anyhow::anyhow!(err))?,
                );
            } else if let Some(found) = crate::nmos::first_routable_ipv4() {
                self.nmos_host = Some(found);
            } else {
                anyhow::bail!(
                    "NMOS_HOST_ADDRESS is unset and this host has no routable IPv4 address"
                );
            }
        }
        let extra_domains = first_env(&["STROM_NMOS_DOMAINS"])?;
        if let Some(raw) = extra_domains {
            self.nmos_domains = raw
                .split(',')
                .map(|entry| PathBuf::from(entry.trim()))
                .filter(|entry| !entry.as_os_str().is_empty())
                .collect();
        }
        Ok(())
    }

    /// NMOS node settings, including the persisted node id next to the flows file.
    pub fn nmos_settings(&self) -> crate::nmos::NmosSettings {
        let id_path = self
            .flows_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("nmos-node.id");
        crate::nmos::NmosSettings {
            enabled: self.nmos_enabled,
            node_id: None,
            id_path: Some(id_path),
            label: self.nmos_label.clone(),
            port: self.port,
            host: self.nmos_host.clone(),
            https: self.tls_cert.is_some() && self.tls_key.is_some(),
            registry: self.nmos_registry.clone(),
            domain_paths: self.nmos_domains.clone(),
            heartbeat_secs: 5,
            seed: self.nmos_seed.clone(),
            tags: self.nmos_tags.clone(),
            dns_sd: self.nmos_dns_sd,
            scan_path: self.mxl_scan_path.clone(),
            output_domain_dir: self.mxl_output_domain_dir.clone(),
            output_domain_id: self.mxl_output_domain_id,
            history_duration_ns: self.mxl_history_duration_ns,
            cleanup_on_exit: self.mxl_cleanup_on_exit,
            query_base: query_base_url(
                self.nmos_registry.as_deref(),
                self.nmos_query_address.as_deref(),
                self.nmos_query_port,
            ),
        }
    }

    /// Returns TLS config paths if both cert and key are provided.
    /// Returns error if only one of cert/key is set.
    pub fn tls_paths(&self) -> anyhow::Result<Option<(&PathBuf, &PathBuf)>> {
        match (&self.tls_cert, &self.tls_key) {
            (Some(cert), Some(key)) => Ok(Some((cert, key))),
            (None, None) => Ok(None),
            (Some(_), None) => anyhow::bail!("--tls-cert provided but --tls-key is missing"),
            (None, Some(_)) => anyhow::bail!("--tls-key provided but --tls-cert is missing"),
        }
    }

    /// Create configuration from explicit values.
    ///
    /// This is the legacy way to construct Config. Use `from_figment()` for config file support.
    pub fn new(
        port: u16,
        data_dir: Option<PathBuf>,
        flows_path: Option<PathBuf>,
        blocks_path: Option<PathBuf>,
        media_path: Option<PathBuf>,
        database_url: Option<String>,
    ) -> anyhow::Result<Self> {
        // Resolve data paths
        let path_config = PathConfig {
            data_dir,
            flows_path,
            blocks_path,
            media_path,
            cef_cache_path: None,
        };
        let data_paths = DataPaths::resolve(path_config)?;

        let mut built = Self {
            port,
            flows_path: data_paths.flows_path,
            blocks_path: data_paths.blocks_path,
            media_path: data_paths.media_path,
            cef_cache_path: data_paths.cef_cache_path,
            database_url,
            log_file: None,
            log_level: None,
            stdout_log_format: LogFormat::default(),
            structured_events: false,
            include_high_frequency_events: false,
            ice_servers: default_ice_servers(),
            ice_transport_policy: default_ice_transport_policy(),
            sap_multicast_addresses: default_sap_multicast_addresses(),
            cors_allowed_origins: Vec::new(),
            tls_cert: None,
            tls_key: None,
            nmos_enabled: true,
            nmos_registry: None,
            nmos_host: None,
            nmos_label: "Strom".to_string(),
            nmos_domains: vec![PathBuf::from(strom_types::mxl::DEFAULT_MXL_DOMAIN)],
            nmos_seed: None,
            nmos_tags: std::collections::HashMap::new(),
            nmos_dns_sd: false,
            nmos_query_address: None,
            nmos_query_port: None,
            mxl_scan_path: PathBuf::from("/Volumes/mxl"),
            mxl_output_domain_dir: None,
            mxl_output_domain_id: None,
            mxl_history_duration_ns: crate::nmos::DEFAULT_HISTORY_DURATION_NS,
            mxl_cleanup_on_exit: false,
            shutdown_timeout_s: 10,
            pool_ports: std::collections::BTreeSet::new(),
            port_lease_ttl_seconds: strom_types::ports::DEFAULT_PORT_LEASE_TTL_SECS,
            probe_before_handout: true,
        };
        built.apply_platform_env()?;
        Ok(built)
    }

    /// Load configuration from environment variables only (legacy support).
    ///
    /// This method is primarily for backward compatibility and tests.
    /// CLI applications should use `Config::new()` with parsed arguments.
    pub fn from_env() -> anyhow::Result<Self> {
        let port = strom_types::env::var_opt("STROM_PORT")
            .and_then(|p| p.parse().ok())
            .unwrap_or(strom_types::DEFAULT_PORT);

        let data_dir = strom_types::env::var_opt("STROM_DATA_DIR").map(PathBuf::from);
        let flows_path = strom_types::env::var_opt("STROM_FLOWS_PATH").map(PathBuf::from);
        let blocks_path = strom_types::env::var_opt("STROM_BLOCKS_PATH").map(PathBuf::from);
        let media_path = strom_types::env::var_opt("STROM_MEDIA_PATH").map(PathBuf::from);
        let database_url = strom_types::env::var_opt("STROM_DATABASE_URL");

        Self::new(
            port,
            data_dir,
            flows_path,
            blocks_path,
            media_path,
            database_url,
        )
    }
}

fn first_env(names: &[&str]) -> anyhow::Result<Option<String>> {
    let mut found: Option<(String, String)> = None;
    for name in names {
        if let Some(value) = strom_types::env::var_opt(name) {
            if let Some((previous, previous_value)) = &found {
                if previous_value != &value {
                    anyhow::bail!("{name}={value} disagrees with {previous}={previous_value}");
                }
            } else {
                found = Some(((*name).to_string(), value));
            }
        }
    }
    Ok(found.map(|(_, value)| value))
}

fn agreed_listen_port() -> anyhow::Result<Option<u16>> {
    let mut found: Option<(&str, u16)> = None;
    for name in ["PORT", "NMOS_PORT", "STROM_PORT", "STROM_SERVER_PORT"] {
        if let Some(raw) = strom_types::env::var_opt(name) {
            let port: u16 = raw
                .parse()
                .map_err(|_| anyhow::anyhow!("{name} is not a valid port: {raw}"))?;
            if let Some((previous, previous_port)) = found {
                if previous_port != port {
                    anyhow::bail!(
                        "{name}={port} disagrees with {previous}={previous_port}; the web UI and NMOS API share one port"
                    );
                }
            } else {
                found = Some((name, port));
            }
        }
    }
    Ok(found.map(|(_, port)| port))
}

fn parse_bool_env(name: &str, raw: &str) -> anyhow::Result<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => anyhow::bail!("{name} must be true or false, got {raw}"),
    }
}

fn parse_tags(raw: &str) -> anyhow::Result<std::collections::HashMap<String, Vec<String>>> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|err| anyhow::anyhow!("NMOS_TAGS is not JSON: {err}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("NMOS_TAGS must be a JSON object"))?;
    let mut tags = std::collections::HashMap::new();
    for (key, entry) in object {
        let values = entry
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("NMOS_TAGS.{key} must be an array of strings"))?;
        let mut strings = Vec::new();
        for item in values {
            let text = item
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("NMOS_TAGS.{key} must contain only strings"))?;
            strings.push(text.to_string());
        }
        tags.insert(key.clone(), strings);
    }
    Ok(tags)
}

fn query_base_url(
    registry: Option<&str>,
    query_address: Option<&str>,
    query_port: Option<u16>,
) -> Option<String> {
    let (default_host, default_port) = registry.and_then(split_http_host)?;
    let host = query_address.unwrap_or(default_host);
    let port = query_port.unwrap_or(default_port.saturating_add(1));
    Some(format!("http://{host}:{port}"))
}

fn split_http_host(url: &str) -> Option<(&str, u16)> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let (host, port) = rest.split_once(':')?;
    let port = port.split('/').next()?.parse().ok()?;
    Some((host, port))
}

fn registry_url(
    address: Option<&str>,
    port: Option<&str>,
    legacy: Option<&str>,
    from_file: Option<&str>,
) -> anyhow::Result<Option<String>> {
    match (address, port) {
        (Some(address), Some(port)) => {
            let port: u16 = port
                .parse()
                .map_err(|_| anyhow::anyhow!("NMOS_REGISTRY_PORT is not a valid port: {port}"))?;
            if address.parse::<std::net::Ipv4Addr>().is_err() {
                anyhow::bail!("NMOS_REGISTRY_ADDRESS must be an IPv4 address, got {address}");
            }
            Ok(Some(format!("http://{address}:{port}")))
        }
        (Some(_), None) => {
            anyhow::bail!("NMOS_REGISTRY_ADDRESS is set but NMOS_REGISTRY_PORT is not")
        }
        (None, Some(_)) => {
            anyhow::bail!("NMOS_REGISTRY_PORT is set but NMOS_REGISTRY_ADDRESS is not")
        }
        (None, None) => Ok(legacy
            .map(str::to_string)
            .or_else(|| from_file.map(str::to_string))
            .filter(|url| !url.is_empty())),
    }
}

impl Default for Config {
    fn default() -> Self {
        // For default, use the path resolution logic
        Self::from_env().unwrap_or_else(|_| {
            // Ultimate fallback (should rarely happen)
            Self {
                port: strom_types::DEFAULT_PORT,
                flows_path: PathBuf::from("flows.json"),
                blocks_path: PathBuf::from("blocks.json"),
                media_path: PathBuf::from("media"),
                cef_cache_path: PathBuf::from("cef-cache"),
                database_url: None,
                log_file: None,
                log_level: None,
                stdout_log_format: LogFormat::default(),
                structured_events: false,
                include_high_frequency_events: false,
                ice_servers: default_ice_servers(),
                ice_transport_policy: default_ice_transport_policy(),
                sap_multicast_addresses: default_sap_multicast_addresses(),
                cors_allowed_origins: Vec::new(),
                tls_cert: None,
                tls_key: None,
                nmos_enabled: true,
                nmos_registry: None,
                nmos_host: None,
                nmos_label: "Strom".to_string(),
                nmos_domains: vec![PathBuf::from(strom_types::mxl::DEFAULT_MXL_DOMAIN)],
                nmos_seed: None,
                nmos_tags: std::collections::HashMap::new(),
                nmos_dns_sd: false,
                nmos_query_address: None,
                nmos_query_port: None,
                mxl_scan_path: PathBuf::from("/Volumes/mxl"),
                mxl_output_domain_dir: None,
                mxl_output_domain_id: None,
                mxl_history_duration_ns: crate::nmos::DEFAULT_HISTORY_DURATION_NS,
                mxl_cleanup_on_exit: false,
                shutdown_timeout_s: 10,
                pool_ports: std::collections::BTreeSet::new(),
                port_lease_ttl_seconds: strom_types::ports::DEFAULT_PORT_LEASE_TTL_SECS,
                probe_before_handout: true,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    #[serial]
    fn test_from_figment_defaults() {
        // Clear any env vars that might have been set by other tests
        std::env::remove_var("STROM_SERVER_PORT");
        std::env::remove_var("STROM_PORT");
        std::env::remove_var("STROM_STORAGE_DATABASE_URL");
        std::env::remove_var("STROM_STORAGE_DATA_DIR");

        // Run in a temp directory to avoid picking up project .strom.toml
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let config =
            Config::from_figment(None, None, None, None, None, None, None, None, None).unwrap();

        // Restore (ignore errors)
        let _ = std::env::set_current_dir(original_dir);

        assert_eq!(config.port, strom_types::DEFAULT_PORT);
        assert!(config.database_url.is_none());
        assert_eq!(config.stdout_log_format, LogFormat::Compact);
        assert!(!config.structured_events);
        assert!(!config.include_high_frequency_events);
    }

    #[test]
    #[serial]
    fn test_from_figment_parses_json_stdout_log_format() {
        std::env::remove_var("STROM_LOGGING_STDOUT_LOG_FORMAT");

        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join(".strom.toml");
        fs::write(&config_file, "[logging]\nstdout_log_format = \"json\"\n").unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let config =
            Config::from_figment(None, None, None, None, None, None, None, None, None).unwrap();

        let _ = std::env::set_current_dir(original_dir);

        assert_eq!(config.stdout_log_format, LogFormat::Json);
    }

    #[test]
    #[serial]
    fn test_from_figment_stdout_log_format_env_var() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        std::env::set_var("STROM_LOGGING_STDOUT_LOG_FORMAT", "json");

        let config =
            Config::from_figment(None, None, None, None, None, None, None, None, None).unwrap();

        let _ = std::env::set_current_dir(&original_dir);
        std::env::remove_var("STROM_LOGGING_STDOUT_LOG_FORMAT");

        assert_eq!(config.stdout_log_format, LogFormat::Json);
    }

    #[test]
    #[serial]
    fn test_from_figment_rejects_invalid_stdout_log_format() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join(".strom.toml");
        fs::write(&config_file, "[logging]\nstdout_log_format = \"yaml\"\n").unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let result = Config::from_figment(None, None, None, None, None, None, None, None, None);

        let _ = std::env::set_current_dir(&original_dir);

        assert!(
            result.is_err(),
            "an unrecognized stdout_log_format must be a clear error, not a silent fallback"
        );
    }

    #[test]
    #[serial]
    fn test_from_figment_parses_structured_events() {
        std::env::remove_var("STROM_LOGGING_STRUCTURED_EVENTS");

        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join(".strom.toml");
        fs::write(&config_file, "[logging]\nstructured_events = true\n").unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let config =
            Config::from_figment(None, None, None, None, None, None, None, None, None).unwrap();

        let _ = std::env::set_current_dir(original_dir);

        assert!(config.structured_events);
        assert!(
            !config.include_high_frequency_events,
            "include_high_frequency_events must default to false independently"
        );
    }

    // Serialized with its neighbours: from_figment reads `.strom.toml` from the
    // process working directory, which those tests move into a TempDir and then
    // delete. Without the lock this test can pick up their config file and try to
    // create a data directory that is being torn down.
    #[test]
    #[serial]
    fn test_from_figment_cli_args_override() {
        let temp_dir = TempDir::new().unwrap();
        let flows = temp_dir.path().join("flows.json");
        let blocks = temp_dir.path().join("blocks.json");

        let config = Config::from_figment(
            Some(9000),
            None,
            Some(flows.clone()),
            Some(blocks.clone()),
            None,
            None,
            Some("postgresql://test".to_string()),
            None,
            None,
        )
        .unwrap();

        assert_eq!(config.port, 9000);
        assert_eq!(config.flows_path, flows);
        assert_eq!(config.blocks_path, blocks);
        assert_eq!(config.database_url, Some("postgresql://test".to_string()));
    }

    #[test]
    #[serial]
    fn test_from_figment_config_file() {
        // Clear any env vars that might interfere
        std::env::remove_var("STROM_SERVER_PORT");
        std::env::remove_var("STROM_STORAGE_DATABASE_URL");

        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join(".strom.toml");

        // Create a test config file
        let config_content = r#"
[server]
port = 7777

[storage]
database_url = "postgresql://localhost/test"
"#;
        fs::write(&config_file, config_content).unwrap();

        // Change to temp directory to make config file discoverable
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let config =
            Config::from_figment(None, None, None, None, None, None, None, None, None).unwrap();

        // Restore original directory (ignore errors if it fails)
        let _ = std::env::set_current_dir(original_dir);

        assert_eq!(config.port, 7777);
        assert_eq!(
            config.database_url,
            Some("postgresql://localhost/test".to_string())
        );
    }

    /// A blank value must not look configured. `Some("")` for the database URL
    /// selected PostgreSQL storage and then failed to connect; blank TLS paths
    /// turned on HTTPS with nothing to serve it with. The environment is
    /// scrubbed in main, but a config file reaches the same fields.
    #[test]
    #[serial]
    fn test_from_figment_ignores_blank_values() {
        std::env::remove_var("STROM_SERVER_PORT");
        std::env::remove_var("STROM_STORAGE_DATABASE_URL");

        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join(".strom.toml");

        let config_content = r#"
[server]
tls_cert = ""
tls_key = "   "

[storage]
database_url = ""
data_dir = ""

[logging]
log_file = ""
log_level = "   "
stdout_log_format = ""
"#;
        fs::write(&config_file, config_content).unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let config =
            Config::from_figment(None, None, None, None, None, None, None, None, None).unwrap();

        let _ = std::env::set_current_dir(original_dir);

        assert_eq!(
            config.database_url, None,
            "a blank database URL must leave JSON file storage in place"
        );
        assert_eq!(config.tls_cert, None);
        assert_eq!(config.tls_key, None);
        assert!(
            config.tls_paths().unwrap().is_none(),
            "blank TLS paths must not enable HTTPS"
        );
        assert_eq!(
            config.log_level, None,
            "a blank log level would build an EnvFilter with no directives, silencing the process"
        );
        assert_eq!(config.log_file, None);
        assert_eq!(
            config.stdout_log_format,
            LogFormat::Compact,
            "a blank stdout_log_format must fall back to the default instead of failing to parse"
        );
        assert!(
            config.flows_path.is_absolute(),
            "a blank data dir must fall back to the default location, got {:?}",
            config.flows_path
        );
    }

    /// The same guard on the CLI layer. This is the shape the OSC failure
    /// actually took: clap reads a set-but-empty `STROM_DATABASE_URL` as a real
    /// value and hands `Some("")` straight to `from_figment`.
    #[test]
    #[serial]
    fn test_from_figment_ignores_blank_cli_database_url() {
        std::env::remove_var("STROM_STORAGE_DATABASE_URL");

        let config = Config::from_figment(
            None,
            None,
            None,
            None,
            None,
            None,
            Some(String::new()),
            Some(PathBuf::new()),
            Some(PathBuf::new()),
        )
        .unwrap();

        assert_eq!(config.database_url, None);
        assert_eq!(config.tls_cert, None);
        assert_eq!(config.tls_key, None);
    }

    #[test]
    #[serial]
    fn test_from_figment_env_vars_override_config_file() {
        // Save and clear any existing env vars
        let original_server_port = std::env::var("STROM_SERVER_PORT").ok();

        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join(".strom.toml");

        // Create a test config file with port 7777
        fs::write(&config_file, "[server]\nport = 7777").unwrap();

        // Set environment variable to override (STROM_SERVER_PORT -> server.port)
        std::env::set_var("STROM_SERVER_PORT", "8888");

        // Change to temp directory
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let config =
            Config::from_figment(None, None, None, None, None, None, None, None, None).unwrap();

        // Restore (restore dir before temp_dir is dropped, ignore errors)
        let _ = std::env::set_current_dir(&original_dir);

        // Restore env vars
        if let Some(port) = original_server_port {
            std::env::set_var("STROM_SERVER_PORT", port);
        } else {
            std::env::remove_var("STROM_SERVER_PORT");
        }

        // Env var should override config file
        assert_eq!(config.port, 8888);
    }

    #[test]
    #[serial]
    fn test_from_figment_cli_overrides_env_and_config() {
        // Save any existing env vars
        let original_server_port = std::env::var("STROM_SERVER_PORT").ok();

        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join(".strom.toml");

        // Create config file with port 7777
        fs::write(&config_file, "[server]\nport = 7777").unwrap();

        // Set env var to 8888
        std::env::set_var("STROM_SERVER_PORT", "8888");

        // Change to temp directory
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        // Pass CLI arg 9999
        let config =
            Config::from_figment(Some(9999), None, None, None, None, None, None, None, None)
                .unwrap();

        // Restore (restore dir before temp_dir is dropped, ignore errors)
        let _ = std::env::set_current_dir(&original_dir);

        // Restore env vars
        if let Some(port) = original_server_port {
            std::env::set_var("STROM_SERVER_PORT", port);
        } else {
            std::env::remove_var("STROM_SERVER_PORT");
        }

        // CLI should have highest priority
        assert_eq!(config.port, 9999);
    }

    #[test]
    #[serial]
    fn test_config_file_with_data_dir() {
        // Clear any env vars that might interfere
        std::env::remove_var("STROM_SERVER_PORT");
        std::env::remove_var("STROM_STORAGE_DATA_DIR");

        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join(".strom.toml");
        let data_dir = temp_dir.path().join("custom_data");

        // Use forward slashes for TOML (works on all platforms)
        let data_dir_str = data_dir.display().to_string().replace('\\', "/");

        let config_content = format!(
            r#"
[server]
port = 8080

[storage]
data_dir = "{}"
"#,
            data_dir_str
        );
        fs::write(&config_file, config_content).unwrap();

        // Change to temp directory
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let config =
            Config::from_figment(None, None, None, None, None, None, None, None, None).unwrap();

        // Restore (ignore errors)
        let _ = std::env::set_current_dir(original_dir);

        assert!(config.flows_path.starts_with(&data_dir));
        assert!(config.blocks_path.starts_with(&data_dir));
    }

    #[test]
    #[serial]
    fn test_ice_servers_env_var() {
        // Save any existing env vars
        let original_ice_servers = std::env::var("STROM_SERVER_ICE_SERVERS").ok();

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        // Set ICE servers env var (comma-separated)
        std::env::set_var(
            "STROM_SERVER_ICE_SERVERS",
            "stun:stun.example.com:3478,turn:user:pass@turn.example.com:3478",
        );

        let config =
            Config::from_figment(None, None, None, None, None, None, None, None, None).unwrap();

        // Restore
        let _ = std::env::set_current_dir(&original_dir);
        if let Some(ice) = original_ice_servers {
            std::env::set_var("STROM_SERVER_ICE_SERVERS", ice);
        } else {
            std::env::remove_var("STROM_SERVER_ICE_SERVERS");
        }

        assert_eq!(config.ice_servers.len(), 2);
        assert_eq!(config.ice_servers[0], "stun:stun.example.com:3478");
        assert_eq!(
            config.ice_servers[1],
            "turn:user:pass@turn.example.com:3478"
        );
    }

    /// Restores an environment variable on drop so a failing assertion cannot
    /// leak it into the next test.
    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, original }
        }

        fn remove(key: &'static str) -> Self {
            let original = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    /// Loads a config from a temp directory, so a `.strom.toml` in the working
    /// directory cannot influence the result.
    fn config_from_env() -> Config {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();
        let config = Config::from_figment(None, None, None, None, None, None, None, None, None);
        let _ = std::env::set_current_dir(original_dir);
        config.unwrap()
    }

    #[test]
    #[serial]
    fn test_ice_transport_policy_env_var() {
        let _guard = EnvGuard::set("STROM_SERVER_ICE_TRANSPORT_POLICY", "relay");

        assert_eq!(config_from_env().ice_transport_policy, "relay");
    }

    #[test]
    #[serial]
    fn test_cors_allowed_origins_env_var() {
        let _guard = EnvGuard::set(
            "STROM_SERVER_CORS_ALLOWED_ORIGINS",
            "https://a.example.com, https://b.example.com",
        );

        assert_eq!(
            config_from_env().cors_allowed_origins,
            vec![
                "https://a.example.com".to_string(),
                "https://b.example.com".to_string()
            ]
        );
    }

    #[test]
    #[serial]
    fn test_sap_multicast_addresses_env_var() {
        let _guard = EnvGuard::set(
            "STROM_DISCOVERY_SAP_MULTICAST_ADDRESSES",
            "239.0.0.1,239.0.0.2",
        );

        assert_eq!(
            config_from_env().sap_multicast_addresses,
            vec!["239.0.0.1".to_string(), "239.0.0.2".to_string()]
        );
    }

    #[test]
    #[serial]
    fn test_logging_env_vars() {
        let _level = EnvGuard::set("STROM_LOGGING_LOG_LEVEL", "debug");
        let _file = EnvGuard::set("STROM_LOGGING_LOG_FILE", "/tmp/strom-env-test.log");

        let config = config_from_env();

        assert_eq!(config.log_level.as_deref(), Some("debug"));
        assert_eq!(
            config.log_file,
            Some(PathBuf::from("/tmp/strom-env-test.log"))
        );
    }

    #[test]
    #[serial]
    fn test_storage_env_vars() {
        let temp_dir = TempDir::new().unwrap();
        let flows = temp_dir.path().join("custom-flows.json");
        let _db = EnvGuard::set(
            "STROM_STORAGE_DATABASE_URL",
            "postgresql://user:pass@db.example.com/strom",
        );
        let _flows = EnvGuard::set("STROM_STORAGE_FLOWS_PATH", flows.to_str().unwrap());

        let config = config_from_env();

        assert_eq!(
            config.database_url.as_deref(),
            Some("postgresql://user:pass@db.example.com/strom")
        );
        assert_eq!(config.flows_path, flows);
    }

    #[test]
    #[serial]
    fn test_tls_env_vars() {
        let _cert = EnvGuard::set("STROM_TLS_CERT", "/etc/strom/cert.pem");
        let _key = EnvGuard::set("STROM_TLS_KEY", "/etc/strom/key.pem");

        let config = config_from_env();

        assert_eq!(config.tls_cert, Some(PathBuf::from("/etc/strom/cert.pem")));
        assert_eq!(config.tls_key, Some(PathBuf::from("/etc/strom/key.pem")));
    }

    #[test]
    #[serial]
    fn test_invalid_port_env_var_is_an_error() {
        let _guard = EnvGuard::set("STROM_SERVER_PORT", "not-a-port");

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();
        let result = Config::from_figment(None, None, None, None, None, None, None, None, None);
        let _ = std::env::set_current_dir(original_dir);

        assert!(result.is_err());
    }

    #[test]
    #[serial]
    fn documented_mixed_port_pool_config_loads() {
        let _ports = EnvGuard::remove("STROM_PORTS");
        let _ttl = EnvGuard::remove("STROM_PORT_LEASE_TTL");
        let temp_dir = TempDir::new().unwrap();
        fs::write(
            temp_dir.path().join(".strom.toml"),
            r#"
[ports]
ports = ["47100-47199", 47250, "47300-47399"]
lease_ttl_seconds = 600
"#,
        )
        .unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let result = Config::from_figment(None, None, None, None, None, None, None, None, None);

        let _ = std::env::set_current_dir(original_dir);
        let config = result.unwrap();
        assert_eq!(config.pool_ports.len(), 201);
        assert!(config.pool_ports.contains(&47100));
        assert!(config.pool_ports.contains(&47250));
        assert!(config.pool_ports.contains(&47399));
    }

    #[test]
    #[serial]
    fn an_unrecognised_probe_setting_fails_config_loading() {
        let _ports = EnvGuard::remove("STROM_PORTS");
        let _ttl = EnvGuard::remove("STROM_PORT_LEASE_TTL");
        let _probe = EnvGuard::set("STROM_PORT_PROBE_BEFORE_HANDOUT", "maybe");
        let err =
            Config::from_figment(None, None, None, None, None, None, None, None, None).unwrap_err();
        assert!(err.to_string().contains("STROM_PORT_PROBE_BEFORE_HANDOUT"));

        let _probe = EnvGuard::set("STROM_PORT_PROBE_BEFORE_HANDOUT", "off");
        let config =
            Config::from_figment(None, None, None, None, None, None, None, None, None).unwrap();
        assert!(!config.probe_before_handout);
    }

    #[test]
    fn port_lease_ttl_is_validated_during_config_loading() {
        assert_eq!(
            port_lease_ttl(None).unwrap(),
            strom_types::ports::DEFAULT_PORT_LEASE_TTL_SECS
        );
        assert!(port_lease_ttl(Some(0)).is_err());
        assert!(port_lease_ttl(Some(strom_types::ports::MAX_PORT_LEASE_TTL_SECS + 1)).is_err());
    }

    #[test]
    fn test_ice_servers_normalization() {
        // Test that URLs with :// are normalized to RFC format (without //)
        assert_eq!(
            normalize_ice_server_url("stun://stun.example.com:3478"),
            "stun:stun.example.com:3478"
        );
        assert_eq!(
            normalize_ice_server_url("turn://user:pass@turn.example.com:3478"),
            "turn:user:pass@turn.example.com:3478"
        );
        assert_eq!(
            normalize_ice_server_url("turns://user:pass@turn.example.com:5349"),
            "turns:user:pass@turn.example.com:5349"
        );
        // Already RFC format should remain unchanged
        assert_eq!(
            normalize_ice_server_url("stun:stun.example.com:3478"),
            "stun:stun.example.com:3478"
        );
        assert_eq!(
            normalize_ice_server_url("turn:user:pass@turn.example.com:3478"),
            "turn:user:pass@turn.example.com:3478"
        );
    }
}
