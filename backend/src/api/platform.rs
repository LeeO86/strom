//! Platform health, metrics, and configuration export.

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use strom_types::flow::Flow;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    #[serde(default)]
    include_secrets: bool,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ExportedSettings {
    pub port: u16,
    pub nmos_enabled: bool,
    pub nmos_label: String,
    pub nmos_seed: Option<String>,
    pub nmos_registry: Option<String>,
    pub nmos_host: Option<String>,
    pub nmos_dns_sd: bool,
    pub mxl_scan_path: String,
    pub mxl_output_domain_dir: Option<String>,
    pub mxl_output_domain_id: Option<String>,
    pub mxl_history_duration_ns: u64,
    pub mxl_cleanup_on_exit: bool,
    /// Always omitted. `include_secrets=true` is accepted and still does not
    /// return API keys, TLS private keys, or the database URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ExportedConfig {
    pub version: u32,
    pub settings: ExportedSettings,
    pub flows: Vec<Flow>,
}

/// Process is alive.
#[utoipa::path(get, path = "/livez", tag = "Platform", responses((status = 200, description = "Alive")))]
pub async fn livez() -> &'static str {
    "ok"
}

/// Ready to serve. When a registry is configured, the node must be registered.
#[utoipa::path(get, path = "/readyz", tag = "Platform", responses((status = 200, description = "Ready"), (status = 503, description = "Not registered")))]
pub async fn readyz(State(state): State<AppState>) -> Response {
    if state.nmos_ready() {
        (StatusCode::OK, "ok").into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "nmos node is not registered",
        )
            .into_response()
    }
}

/// Prometheus text. Metric names use the `strom_` prefix.
#[utoipa::path(get, path = "/metrics", tag = "Platform", responses((status = 200, description = "Prometheus text")))]
pub async fn metrics(State(state): State<AppState>) -> Response {
    let registered = match state.nmos() {
        Some(node) if node.registry_required() => u8::from(node.is_registered()),
        Some(_) => 1,
        None => 0,
    };
    let running = state
        .get_flows()
        .await
        .into_iter()
        .filter(|flow| flow.running)
        .count();
    let wedged = crate::gst::pipeline::WEDGED_PIPELINES.load(std::sync::atomic::Ordering::Relaxed);
    let body = format!(
        "# HELP strom_up Process is running.\n# TYPE strom_up gauge\nstrom_up 1\n\
         # HELP strom_nmos_registered NMOS node is registered, or no registry is configured.\n\
         # TYPE strom_nmos_registered gauge\nstrom_nmos_registered {registered}\n\
         # HELP strom_flows_running Flows currently running.\n\
         # TYPE strom_flows_running gauge\nstrom_flows_running {running}\n\
         # HELP strom_pipelines_wedged_total Pipelines whose stop never completed; they keep their thread until the process restarts.\n\
         # TYPE strom_pipelines_wedged_total counter\nstrom_pipelines_wedged_total {wedged}\n"
    );
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

/// Export settings and flows. Secrets are omitted. `include_secrets=true` is accepted and still omits them.
#[utoipa::path(
    get,
    path = "/api/v1/config/export",
    tag = "Platform",
    params(("include_secrets" = Option<bool>, Query, description = "Accepted for compatibility. Secrets are still omitted.")),
    responses((status = 200, description = "Configuration document", body = ExportedConfig))
)]
pub async fn export_config(
    State(state): State<AppState>,
    Query(query): Query<ExportQuery>,
) -> Json<ExportedConfig> {
    Json(build_export(&state, query.include_secrets).await)
}

/// Replace stored flows from an export document. Running flows are stopped first.
#[utoipa::path(
    post,
    path = "/api/v1/config/import",
    tag = "Platform",
    request_body = ExportedConfig,
    responses((status = 200, description = "Imported"), (status = 400, description = "Rejected"))
)]
pub async fn import_config(
    State(state): State<AppState>,
    Json(document): Json<ExportedConfig>,
) -> Result<StatusCode, (StatusCode, String)> {
    if document.version != 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("unsupported config version {}", document.version),
        ));
    }
    state.stop_running_flows().await;
    let existing = state.get_flows().await;
    for flow in existing {
        let _ = state.delete_flow(&flow.id).await;
    }
    for flow in document.flows {
        if let Err(err) = state.upsert_flow(flow).await {
            return Err((StatusCode::BAD_REQUEST, err.to_string()));
        }
    }
    Ok(StatusCode::OK)
}

async fn build_export(state: &AppState, include_secrets: bool) -> ExportedConfig {
    let node = state.nmos();
    let settings = node.as_ref().map(|node| node.settings().clone());
    let _ = include_secrets;
    ExportedConfig {
        version: 1,
        settings: ExportedSettings {
            port: settings.as_ref().map(|s| s.port).unwrap_or(0),
            nmos_enabled: settings.as_ref().is_some_and(|s| s.enabled),
            nmos_label: settings
                .as_ref()
                .map(|s| s.label.clone())
                .unwrap_or_default(),
            nmos_seed: settings.as_ref().and_then(|s| s.seed.clone()),
            nmos_registry: settings.as_ref().and_then(|s| s.registry.clone()),
            nmos_host: settings.as_ref().and_then(|s| s.host.clone()),
            nmos_dns_sd: settings.as_ref().is_some_and(|s| s.dns_sd),
            mxl_scan_path: settings
                .as_ref()
                .map(|s| s.scan_path.display().to_string())
                .unwrap_or_default(),
            mxl_output_domain_dir: settings.as_ref().and_then(|s| {
                s.output_domain_dir
                    .as_ref()
                    .map(|p| p.display().to_string())
            }),
            mxl_output_domain_id: settings
                .as_ref()
                .and_then(|s| s.output_domain_id.map(|id| id.to_string())),
            mxl_history_duration_ns: settings
                .as_ref()
                .map(|s| s.history_duration_ns)
                .unwrap_or(0),
            mxl_cleanup_on_exit: settings.as_ref().is_some_and(|s| s.cleanup_on_exit),
            database_url: None,
        },
        flows: state.get_flows().await,
    }
}
