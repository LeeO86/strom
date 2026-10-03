# Strom platform contract (v1.0.0)

The code is the source of truth. This file is the stable contract for settings, ports, exit codes, and the platform HTTP API. A breaking change to this contract is v2.

## Configuration

Precedence is environment, then `.strom.toml`, then defaults. Unknown environment variables are ignored. An invalid value exits **78** and prints the reason.

State Strom writes (flows, the node id file, port reservations, media, the CEF cache) lives under one directory. The default is `/config`. `CONFIG_DIR` sets it. `STROM_DATA_DIR` and `STROM_STORAGE_DATA_DIR` are aliases used only when `CONFIG_DIR` is unset. A command-line data directory wins over all of them. If `/config` cannot be created, the process uses `$TMPDIR/strom-config` and logs that. The published image creates `/config` owned by uid 1000, so the platform default holds.

Secrets (API keys, TLS private keys, database URLs) are not written to logs and are not included in config export.

## Settings

| Name | Default | Alias | Meaning |
| --- | --- | --- | --- |
| `CONFIG_DIR` | `/config` | `STROM_DATA_DIR`, `STROM_STORAGE_DATA_DIR` | State directory |
| `PORT` | `8080` | `NMOS_PORT`, `STROM_PORT`, `STROM_SERVER_PORT` | HTTP port for the UI, REST API, and NMOS |
| `NMOS_SEED` | unset | | UUIDv5 seed for the node id and the default output domain id |
| `NMOS_LABEL` | `Strom` | `STROM_NMOS_LABEL` | Node label and device label prefix |
| `NMOS_TAGS` | `{}` | | JSON object of tag name to array of strings, copied onto the node and each device |
| `NMOS_DNS_SD` | `false` | | When false, no registry browse and no `_nmos-node` advertisement |
| `NMOS_ENABLED` | `true` | `STROM_NMOS_ENABLED` | Register and announce. The HTTP APIs stay up either way |
| `NMOS_REGISTRY_ADDRESS` | unset | `STROM_NMOS_REGISTRY` is a full `http://host:port` URL | Registration API address. Must be an IPv4 literal |
| `NMOS_REGISTRY_PORT` | unset | | Registration API port. Required when `NMOS_REGISTRY_ADDRESS` is set |
| `NMOS_QUERY_ADDRESS` | registration address | | Query API address |
| `NMOS_QUERY_PORT` | registration port + 1 | | Query API port |
| `NMOS_HOST_ADDRESS` | first non-loopback IPv4 | `STROM_NMOS_HOST` | Address in the IS-04 `href` and `api.endpoints[].host` |
| `MXL_DOMAIN_SCAN_PATH` | `/Volumes/mxl` | | Parent of domain directories, mirrors included |
| `MXL_OUTPUT_DOMAIN_DIR` | unset | | This function's output domain. Created if missing |
| `MXL_OUTPUT_DOMAIN_ID` | UUIDv5 of `NMOS_SEED` when the seed is set | | Must match an existing `domain_def.json`. A different id is an error and the file is not overwritten |
| `MXL_HISTORY_DURATION_NS` | `200000000` | | Written to `options.json` only when that file is created |
| `MXL_CLEANUP_ON_EXIT` | `false` | | On SIGTERM, delete only `MXL_OUTPUT_DOMAIN_DIR` |
| `SHUTDOWN_TIMEOUT_S` | `10` | | Bound on stop, deregister, and domain removal |
| `STROM_NMOS_DOMAINS` | `/dev/shm/mxl` | | Extra domain directories, comma-separated, in addition to the scan |

`NMOS_PORT` is the same listen port as `PORT`. If two of `PORT`, `NMOS_PORT`, `STROM_PORT`, and `STROM_SERVER_PORT` disagree, the process exits 78.

There is no NMOS WebSocket. Strom is a Node, not a registry. IS-04 and IS-05 are HTTP on `PORT`.

`NMOS_HOST_ADDRESS` must be a routable IPv4 literal. A hostname, `0.0.0.0`, and `127.0.0.1` are rejected. The IS-04 `hostname` field remains the machine name; it is not the address other systems use to connect.

ICE candidates and SRT addresses are the values configured on each block. Strom does not rewrite them from `NMOS_HOST_ADDRESS`.

## Ports and exit codes

| Port | Bind |
| --- | --- |
| `PORT` (default 8080) | HTTP: UI, `/api`, `/x-nmos`, `/livez`, `/readyz`, `/metrics` |

| Code | When |
| --- | --- |
| 0 | Clean exit without SIGTERM |
| 75 | The HTTP port cannot be bound |
| 78 | Configuration is invalid, or the output domain id does not match the existing `domain_def.json` |
| 143 | SIGTERM after shutdown |

A media port that a flow fails to bind is a flow error. The process keeps serving the other flows.

## HTTP

| Method | Path | Auth |
| --- | --- | --- |
| GET | `/livez` | no |
| GET | `/readyz` | no. 200 when the process is serving. If a registry is configured, 200 only after the node has registered |
| GET | `/metrics` | no. Prometheus text, prefix `strom_` |
| GET | `/health` | no. Same as `/livez` |
| GET | `/api/v1/config/export` | same as the rest of `/api` |
| POST | `/api/v1/config/import` | same as the rest of `/api` |
| * | `/x-nmos/...` | no. IS-04 v1.3 and IS-05 v1.2 |

`include_secrets=true` on export is accepted and still omits database URLs, API keys, and TLS keys.

Import replaces stored flows. It does not change process environment.

## NMOS and MXL

`NMOS_SEED` fixes the node id: `UUIDv5(8c76aff5-a8f9-53ce-97d0-766f1074b699, seed)`. The output domain id, when `MXL_OUTPUT_DOMAIN_ID` is unset, is `UUIDv5` of `seed + "/mxl-output-domain"` in that same namespace. Device, source, flow, sender, and receiver ids are UUIDv5 of the node id, so they stay stable across restarts.

`NMOS_DNS_SD=false` does not open mDNS. A configured registry URL is used directly. The image does not require Avahi for NMOS.

`MXL_DOMAIN_SCAN_PATH` is scanned for child directories that contain `domain_def.json`, including `mirror-*`. Strom does not write into those directories.

On SIGTERM, within `SHUTDOWN_TIMEOUT_S`: running flows are stopped, the node is DELETED from the registry, and if `MXL_CLEANUP_ON_EXIT=true` the output domain directory is removed. The process then exits 143.

## Image

`ghcr.io/leeo86/strom` and `ghcr.io/leeo86/strom-full` run as uid 1000. On `main`, CI pushes `git-<sha7>` and `nightly-dev`. On a `vX.Y.Z` tag it pushes `X.Y.Z`, `X.Y`, and `X`. The `:mxl` tag is not moved. Labels include `org.opencontainers.image.source`, `org.opencontainers.image.revision`, `org.opencontainers.image.licenses`, and `io.dmf.mxl.revision`.

NVENC uses the NVIDIA devices the runtime mounts. The process does not need to be root for that. NDI mDNS (Avahi) starts only when the container is root.
