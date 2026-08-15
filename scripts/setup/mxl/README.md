# MXL Setup

Scripts and notes for running Strom against a Media eXchange Layer (MXL) domain.

MXL is Apache-2.0: [dmf-mxl/mxl](https://github.com/dmf-mxl/mxl). Strom does not vendor the SDK. The `mxl` cargo feature compiles the MXL blocks; `gstmxl` is loaded at runtime because `gst-mxl-rs` pins gstreamer-rs 0.24 and Strom uses 0.25.

Code is the source of truth — this may have drifted; read the code for the current implementation.

## What you need

1. **libmxl.so** — MXL SDK shared library
2. **libgstmxl.so** — GStreamer plugin providing `mxlsrc` / `mxlsink`
3. A domain directory on tmpfs, typically `/dev/shm/mxl`
4. For the GPU path: NVIDIA Container Toolkit and `--gpus all` (see [DOCKER_GPU_SETUP.md](../../../docs/DOCKER_GPU_SETUP.md))

The GPU element unit test is opt-in (`STROM_V210GL_GPU_TEST=1`) because an RGB10A2
`glupload` can abort on software GL stacks.

## Host install

```bash
./install-mxl-sdk.sh
./verify-mxl.sh
```

`install-mxl-sdk.sh` clones [dmf-mxl/mxl](https://github.com/dmf-mxl/mxl) (default tag `v1.1.0-beta-1`, the first release that ships `gst-mxl-rs`), builds `libmxl` and `libgstmxl`, and installs them under `/usr/local`.

## Docker PoC (GPU host with existing MXL media functions)

The Strom image built with `--features no-gui,efp,mxl` contains the MXL blocks and the `v210glupload` / `v210gldownload` elements. It does **not** embed `libmxl.so` by default (CI/image size). Bind-mount the host SDK and the MXL domain:

```bash
# Create the domain on the host if your media functions use the default path
mkdir -p /dev/shm/mxl

docker run -d --name strom \
  --gpus all \
  --ipc=host \
  -e NVIDIA_DRIVER_CAPABILITIES=all \
  -e GST_GL_WINDOW=egl-device \
  -e GST_GL_PLATFORM=egl \
  -e LD_LIBRARY_PATH=/usr/local/lib \
  -e GST_PLUGIN_PATH=/usr/local/lib/gstreamer-1.0 \
  -v /dev/shm/mxl:/dev/shm/mxl \
  -v /usr/local/lib/libmxl.so:/usr/local/lib/libmxl.so:ro \
  -v /usr/local/lib/gstreamer-1.0/libgstmxl.so:/usr/local/lib/gstreamer-1.0/libgstmxl.so:ro \
  -p 8080:8080 \
  -v "$(pwd)/data:/data" \
  strom-mxl:local
```

Adjust the two library bind-mounts to wherever your working MXL install lives. `--ipc=host` plus the `/dev/shm/mxl` bind is what lets the container see grains published by host media functions.

Build the image from this branch:

```bash
docker build -t strom-mxl:local --build-arg STROM_FEATURES=no-gui,efp,mxl .
```

## Vision mixer

Keep `gl_download=false` (the default) so PGM stays in `memory:GLMemory` for `v210gldownload`. A parallel WHEP/encoder branch can consume the same PGM pad when the encoder accepts GL memory (NVENC interop).

Interlaced MXL video is rejected on both backends (progressive-only caps). If MXL later publishes progressive, no extra block is needed. If a flow stays interlaced, insert a deinterlace block on the CPU path (same pattern as MPEG-TS/SRT input) — that is not wired automatically.
