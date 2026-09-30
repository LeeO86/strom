# Connecting MXL flows with NMOS

> **Code is the source of truth.** This guide describes intended behaviour and may have drifted from the current implementation. When in doubt, read the code.

Strom is an NMOS **Node** for [BCP-007-03](https://specs.amwa.tv/bcp-007-03/releases/v1.0.0/docs/NMOS-With-MXL.html). It serves IS-04 v1.3 and IS-05 v1.2 on the same port as the web UI. It does not run a registry. Use a separate IS-04 registry, for example [nmos-cpp](https://github.com/sony/nmos-cpp)'s registry.

A controller discovers MXL senders and receivers through the registry, then connects them with `mxl_domain_id` and `mxl_flow_id`. There is no SDP.

## What gets advertised

One Node per Strom process. Each flow that contains MXL blocks is one Device.

| Block | NMOS resources |
|---|---|
| MXL Video Output, MXL Audio Output | Source, Flow, Sender (`urn:x-nmos:transport:mxl`) |
| MXL Video Input, MXL Audio Input | Receiver |

The domain id is the UUID in `domain_def.json`, not the mount path. Example:

```json
{
  "id": "3310f209-9351-47c0-b9a2-14c59b6a4c23",
  "label": "Red Studio",
  "description": "MXL domain",
  "tags": {}
}
```

Place that file in the domain directory (the default path is `/dev/shm/mxl`).

## Configuration

In `.strom.toml`:

```toml
[nmos]
enabled = true
registry = "http://192.0.2.10:3210"
# host = "192.0.2.10"          # address controllers use to reach this node
# domains = ["/dev/shm/mxl"]   # directories that contain domain_def.json
# label = "Strom"
```

Or environment variables: `STROM_NMOS_ENABLED`, `STROM_NMOS_REGISTRY`, `STROM_NMOS_HOST`, `STROM_NMOS_LABEL`, `STROM_NMOS_DOMAINS` (comma-separated).

`enabled = false` still serves `/x-nmos` but does not advertise or register. The Node API is not behind Strom's API key; that is how NMOS controllers reach it. Do not expose the Strom port on an untrusted network without a firewall.

Docker usually blocks mDNS. Set `registry` explicitly. `--network host` is what makes mDNS discovery of the registry work.

## Activation

An immediate IS-05 activation with `master_enable: true` writes the domain path and flow id onto the block and starts that Strom flow. `master_enable: false` stops the flow so the MXL writer or reader is destroyed. Changing the connection restarts the whole flow, including anything else in it.

Senders accept `auto` for both parameters. With one visible domain, `auto` selects it. Receivers accept `auto` for the domain only, and reject `auto` for the flow id.

Check the live node:

```bash
curl -s http://127.0.0.1:8080/x-nmos/node/v1.3/senders | head
curl -s http://127.0.0.1:8080/x-nmos/connection/v1.2/single/senders/<id>/active
```

`active.transport_params[0]` should show UUIDs, not `auto`, after a successful activation. `master_enable` is true only while that flow is running.
