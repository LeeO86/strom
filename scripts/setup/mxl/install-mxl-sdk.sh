#!/usr/bin/env bash
# Build and install libmxl + libgstmxl from dmf-mxl/mxl (Apache-2.0).
set -euo pipefail

MXL_REF="${MXL_REF:-v1.1.0-beta-1}"
MXL_REPO="${MXL_REPO:-https://github.com/dmf-mxl/mxl.git}"
PREFIX="${INSTALL_PREFIX:-/usr/local}"
WORK_DIR="${WORK_DIR:-/tmp/mxl-sdk-build}"
JOBS="${JOBS:-$(nproc)}"

echo "==> Cloning ${MXL_REPO} @ ${MXL_REF}"
rm -rf "${WORK_DIR}"
git clone --depth 1 --branch "${MXL_REF}" "${MXL_REPO}" "${WORK_DIR}"

echo "==> Building libmxl (CMake Release)"
cmake -S "${WORK_DIR}" -B "${WORK_DIR}/build" \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="${PREFIX}" \
  -DBUILD_DOCS=OFF \
  -DBUILD_TESTS=OFF \
  -DBUILD_TOOLS=ON
cmake --build "${WORK_DIR}/build" -j "${JOBS}"
cmake --install "${WORK_DIR}/build"

echo "==> Building gst-mxl-rs plugin"
if [[ -d "${WORK_DIR}/rust/gst-mxl-rs" ]]; then
  (
    cd "${WORK_DIR}/rust"
    cargo build --release -p gst-mxl-rs
  )
  PLUGIN_DIR="${PREFIX}/lib/gstreamer-1.0"
  mkdir -p "${PLUGIN_DIR}"
  if [[ -f "${WORK_DIR}/rust/target/release/libgstmxl.so" ]]; then
    install -m0755 "${WORK_DIR}/rust/target/release/libgstmxl.so" "${PLUGIN_DIR}/libgstmxl.so"
  else
    echo "warning: libgstmxl.so not found after cargo build" >&2
  fi
else
  echo "warning: rust/gst-mxl-rs missing on this ref — install the plugin separately" >&2
fi

ldconfig || true
echo "==> Installed MXL SDK to ${PREFIX}"
echo "    export LD_LIBRARY_PATH=${PREFIX}/lib:\${LD_LIBRARY_PATH}"
echo "    export GST_PLUGIN_PATH=${PREFIX}/lib/gstreamer-1.0:\${GST_PLUGIN_PATH}"
