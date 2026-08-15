# AGENTS.md

## Cursor Cloud specific instructions

These notes cover non-obvious gotchas for developing Strom in the Cursor Cloud
environment. Standard build/run/test commands live in `docs/DEVELOPMENT.md`,
`CLAUDE.md`, and `.github/workflows/ci.yml` — read those for the canonical
commands. The environment update script already installs the GStreamer stack,
pins the Rust toolchain, adds the `wasm32-unknown-unknown` target, and installs
`trunk`, so you do not need to install dependencies yourself.

### Rust toolchain is pinned to 1.95.0 (do not use latest stable)

The toolchain is intentionally pinned to `1.95.0`, not the latest stable:

- A transitive git dependency (`gst-plugins-lsp`) requires the `edition2024`
  Cargo feature, so Rust must be `>= 1.85`.
- `sysinfo` (pinned in `Cargo.lock`) requires `rustc >= 1.95`.
- Rust `>= 1.96` enables the `float_literal_f32_fallback` future-incompatibility
  lint, which turns ~187 frontend warnings into errors under
  `cargo clippy -- -D warnings` (the code itself still compiles). `1.95.0` is the
  only version that satisfies all three constraints.

If a future dependency bump requires a newer compiler, bump the pin in the
update script — do not just run `rustup update stable`.

### The VM sets `NO_COLOR=1`, which breaks `trunk`

The VM exports `NO_COLOR=1`. `trunk`'s CLI rejects it
(`invalid value '1' for '--no-color'`). Because the backend `build.rs`
shells out to `trunk build`, this breaks `cargo build`, `cargo run`, and
`cargo clippy --all-targets` on the backend, as well as running `trunk`
directly. Run those with `NO_COLOR` set to a valid boolean, e.g.:

```bash
NO_COLOR=true cargo build --features efp
NO_COLOR=true cargo run -- --headless
NO_COLOR=true trunk build --release   # from frontend/
```

`~/.bashrc` exports `NO_COLOR=true` for interactive shells, but that does not
carry across fresh VMs, so keep prefixing non-interactive build commands.

### Web UI is built automatically by the backend build

`backend/build.rs` runs `trunk build --release` (output → `backend/dist`, embedded
via rust-embed) whenever `trunk` is on `PATH`. So `cargo build`/`cargo run`
produces the full embedded web UI — no separate frontend build step is needed
before running the server. For fast frontend-only iteration use `trunk serve` in
`frontend/` (dev server on `:8095`).

### Running the server headless

This is a headless VM with no default display. Run the backend with `--headless`
so it serves the REST API + embedded web UI without trying to open a native egui
window:

```bash
NO_COLOR=true ./target/debug/strom --headless --port 8080
```

Then the web UI and API are at `http://localhost:8080` (Swagger at
`/swagger-ui`). Omitting `--headless` launches the native GUI, which needs a
display (available via the computer-use/VNC session).

### Benign runtime warnings

- `NVML initialization failed ... libnvidia-ml.so.1` / `nvidia-smi also
  unavailable`: there is no GPU in the VM, so GPU monitoring is disabled. Harmless.
- `Failed to set High priority for streaming thread ... CAP_SYS_NICE`: the debug
  binary lacks `CAP_SYS_NICE`, so streaming threads run at normal priority. Harmless.
