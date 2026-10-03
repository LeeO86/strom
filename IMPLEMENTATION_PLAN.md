# Platform guideline G1–G14

Audit of this repository against the MXL platform guideline. The code is the source of truth; this table records status for the v1.0.0 contract.

| ID | Requirement | Status | Evidence |
| --- | --- | --- | --- |
| G1 | Env, then file, then defaults. Invalid values exit 78. State under one directory, default `/config`. Secrets not logged. | met | `backend/src/config.rs` (`apply_platform_env`, `CONFIG_DIR`), `backend/src/main.rs` exit 78, `backend/src/paths.rs` default `/config` |
| G2 | Scan `MXL_DOMAIN_SCAN_PATH` (default `/Volumes/mxl`), including mirrors. Own output domain created once. Do not overwrite a different domain id. `history_duration` configurable. | met | `backend/src/nmos/domain.rs` `scan_domain_root`, `ensure_output_domain` |
| G3 | `NMOS_SEED` derives UUIDv5 ids. `NMOS_LABEL` is the node label and device prefix. `NMOS_TAGS` on node and device. Group hints stay. | met | `backend/src/nmos/settings.rs` `node_id_from_seed`, `backend/src/nmos/node.rs` tags and device label |
| G4 | Registry address and port. Query address defaults to the registry, query port to registration port + 1. `NMOS_DNS_SD` defaults false and disables browse and advertisement. No Avahi required. | met | `backend/src/nmos/register.rs` `Discovery::start`, `backend/src/config.rs` `query_base_url` |
| G5 | Announced NMOS host is an IPv4 literal from `NMOS_HOST_ADDRESS` (alias `STROM_NMOS_HOST`). No hostname, `0.0.0.0`, or `127.0.0.1`. | met | `backend/src/nmos/node.rs` `require_announce_ipv4` |
| G6 | One configurable HTTP port (`PORT`, aliases `NMOS_PORT`, `STROM_PORT`, `STROM_SERVER_PORT`). Bind failure exits 75. | met | `backend/src/config.rs` `agreed_listen_port`, `backend/src/main.rs` `ensure_port_free` |
| G7 | `/livez`, `/readyz` (registered when a registry is set), `/metrics` with `strom_` prefix. | met | `backend/src/api/platform.rs` |
| G8 | SIGTERM stops flows, DELETEs the node, optionally removes the output domain, exits 143 within `SHUTDOWN_TIMEOUT_S`. | met | `backend/src/main.rs` signal task, `backend/src/nmos/mod.rs` `shutdown` |
| G9 | IS-05 BCP-007-03 parameters and `master_enable: false`. Active connection is the saved flow, so it survives a restart. | met | `backend/src/nmos/node.rs`, `backend/src/state/mod.rs` `apply_nmos_mxl` |
| G10 | `GET/POST /api/v1/config/export` and `/import`. Secrets omitted. Existing `/api` routes stay. | met | `backend/src/api/platform.rs` |
| G11 | GHCR tags `git-<sha7>` and `nightly-dev` on main; `X.Y.Z`, `X.Y`, `X` on `vX.Y.Z`. No moving `:mxl` tag. Image uid 1000. OCI labels including `io.dmf.mxl.revision`. | met | `.github/workflows/ci.yml`, `Dockerfile` |
| G12 | `deploy/strom.yaml` shows pod network, probes, MXL hostPath, `/config`, no hostIPC. | met | `deploy/strom.yaml` |
| G13 | README settings, ports, exit codes, API list. CHANGELOG 1.0.0. This file and `SPECIFICATION.md`. | met | `README.md`, `docs/CHANGELOG.md`, `SPECIFICATION.md` |
| G14 | Unit tests for parsing and domain/seed behaviour. Integration test covers register then SIGTERM cleanup path (`shutdown`). | met | `backend/src/nmos/mod.rs` `shutdown_unregisters_the_node_and_removes_its_domain` |

N/A:

- A separate NMOS WebSocket on `NMOS_PORT+1`. Strom is a Node, not a registry, and serves IS-04/IS-05 HTTP on the same port as the web UI.
- JSON config file. The existing file is `.strom.toml`. Replacing it would drop deployed files. Environment variables are the platform contract.
- Announcing ICE or SRT from `NMOS_HOST_ADDRESS`. Those addresses are the block properties the operator set. The address Strom itself publishes for the node is `NMOS_HOST_ADDRESS`.
