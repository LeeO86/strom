# MXL Setup

Scripts and notes for running Strom against a Media eXchange Layer (MXL) domain.

MXL is Apache-2.0: [dmf-mxl/mxl](https://github.com/dmf-mxl/mxl). Strom does not vendor the SDK source. The `mxl` cargo feature compiles the MXL blocks; `gstmxl` is loaded at runtime because `gst-mxl-rs` pins gstreamer-rs 0.24 and Strom uses 0.25.

The Docker image CI publishes to GHCR **does** bake `libmxl.so` and `libgstmxl.so`. You only need the host install script when running Strom on the host, or when building an image without the MXL stage.

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

`install-mxl-sdk.sh` clones [dmf-mxl/mxl](https://github.com/dmf-mxl/mxl) (default tag `v1.1.0-beta-1`, the first release that ships `gst-mxl-rs`), installs the CMake CONFIG deps via vcpkg (`stduuid`, `spdlog`, `fmt`, `picojson`), builds `libmxl` and `libgstmxl`, and installs them under `/usr/local`. The script fails if either `.so` is missing.

## GHCR image (recommended for GPU host tests)

CI builds linux/amd64 with `--features no-gui,efp,mxl` and bakes `libmxl.so` + `libgstmxl.so`. Pull it instead of building locally. `strom-full` is the same image plus CEF (`gstcefsrc`) and Xvfb — use that when Open Live HTML graphics sources need `cefsrc`.

```bash
docker pull ghcr.io/leeo86/strom:mxl
docker pull ghcr.io/leeo86/strom-full:mxl
# or pin a commit / PR:
# docker pull ghcr.io/leeo86/strom:mxl-<shortsha>
# docker pull ghcr.io/leeo86/strom-full:mxl-<shortsha>
# docker pull ghcr.io/leeo86/strom:pr-<n>
# docker pull ghcr.io/leeo86/strom-full:pr-<n>
```

The first package GitHub creates is often **private**. Either `docker login ghcr.io` with a token that has `read:packages`, or set the package public (GitHub → Packages → `strom` → Package settings → Change visibility).

On a host that already runs MXL media functions, bind the domain only — do **not** bind-mount host `.so` files over the baked ones, and do **not** pass `--ipc=host`:

```bash
mkdir -p /dev/shm/mxl

docker run -d --name strom \
  --gpus all \
  -e NVIDIA_DRIVER_CAPABILITIES=all \
  -e GST_GL_WINDOW=egl-device \
  -e GST_GL_PLATFORM=egl \
  -v /dev/shm/mxl:/dev/shm/mxl \
  -p 8080:8080 \
  -v "$(pwd)/data:/data" \
  ghcr.io/leeo86/strom:mxl
  # or ghcr.io/leeo86/strom-full:mxl when HTML graphics / cefsrc is required
```

If the host domain is not `/dev/shm/mxl` (for example a separate tmpfs), bind that path onto the container path the blocks use (`domain`, default `/dev/shm/mxl`):

```bash
  -v /path/on/host/domain:/dev/shm/mxl
```

MXL shares grains by mmap of files in the domain directory. `--ipc=host` is not required and is harmful: `mxlsrc` will still open `flow_def.json` and negotiate caps, then emit zero buffers and leave the pipeline stuck in PAUSED (pending PLAYING). CBC media-function containers use the default private IPC namespace with the same bind mount.

`mxlsink` live output needs `sync=false` `async=false` (Strom MXL output blocks now default to that). With both at GStreamer defaults the sink creates the flow directory, receives buffers, writes no grains, and leaks the flow on teardown.

If `mxlsink` fails with `Failed to load MXL API` and a path under `/tmp/mxl-sdk-build/build/Linux-Clang-Release/`, the plugin is an image built before the runtime-path fix. Pull a newer `ghcr.io/leeo86/strom:mxl` / `ghcr.io/leeo86/strom-full:mxl` tag, or as a one-shot workaround inside the running container:

```bash
mkdir -p /tmp/mxl-sdk-build/build/Linux-Clang-Release/lib
ln -sf /usr/local/lib/libmxl.so /tmp/mxl-sdk-build/build/Linux-Clang-Release/lib/libmxl.so
```

To build the same image locally:

```bash
docker build -t strom-mxl:local --build-arg STROM_FEATURES=no-gui,efp,mxl .
```

To layer CEF/Xvfb on that local base (Open Live HTML graphics):

```bash
docker build -f docker/strom-full/Dockerfile \
  --build-arg BASE_IMAGE=strom-mxl \
  --build-arg VERSION=local \
  --build-arg TARGETARCH=amd64 \
  -t strom-mxl-full:local \
  docker/strom-full
```

## Vision mixer

Keep `gl_download=false` (the default) so PGM stays in `memory:GLMemory` for `v210gldownload`. A parallel WHEP/encoder branch can consume the same PGM pad when the encoder accepts GL memory (NVENC interop).

Interlaced MXL video is rejected on both backends (progressive-only caps). If MXL later publishes progressive, no extra block is needed. If a flow stays interlaced, insert a deinterlace block on the CPU path (same pattern as MPEG-TS/SRT input) — that is not wired automatically.
