#!/usr/bin/env bash
# Build and install libmxl + libgstmxl from dmf-mxl/mxl (Apache-2.0).
#
# libmxl's CMake CONFIG packages come from vcpkg (stduuid, spdlog, fmt,
# picojson). Tests/tools dependencies are omitted. gst-mxl-rs is built with
# mxl-not-built so it does not re-run MXL's Clang CMake presets.
set -euo pipefail

MXL_REF="${MXL_REF:-v1.1.0-beta-1}"
MXL_REPO="${MXL_REPO:-https://github.com/dmf-mxl/mxl.git}"
PREFIX="${INSTALL_PREFIX:-/usr/local}"
WORK_DIR="${WORK_DIR:-/tmp/mxl-sdk-build}"
VCPKG_ROOT="${VCPKG_ROOT:-${HOME}/vcpkg}"
VCPKG_BASELINE="${VCPKG_BASELINE:-4002e3abc6d3e468c73d2d9777a7dd96af5dc224}"
JOBS="${JOBS:-$(nproc)}"

apt_install() {
  local pkgs=(
    git cmake ninja-build build-essential curl zip unzip tar
    pkg-config python3 ca-certificates libclang-dev
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
  )
  if [[ "$(id -u)" -eq 0 ]]; then
    apt-get update
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "${pkgs[@]}"
  elif command -v sudo >/dev/null; then
    sudo apt-get update
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "${pkgs[@]}"
  else
    echo "error: missing build packages and cannot run apt (not root, no sudo)" >&2
    exit 1
  fi
}

need_apt=0
for cmd in git cmake ninja curl zip python3 pkg-config; do
  command -v "${cmd}" >/dev/null || need_apt=1
done
if [[ ! -e /usr/include/gstreamer-1.0/gst/gst.h ]]; then
  need_apt=1
fi
if [[ "${need_apt}" -eq 1 ]]; then
  if [[ -f /etc/debian_version ]]; then
    apt_install
  else
    echo "error: install git, cmake, ninja, zip, pkg-config, libclang, and GStreamer dev packages first" >&2
    exit 1
  fi
fi

if ! command -v cargo >/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi
# rustup may have been installed in this shell or a previous one
if [[ -f "${HOME}/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  . "${HOME}/.cargo/env"
fi
if ! command -v cargo >/dev/null; then
  echo "error: cargo not found after rustup install" >&2
  exit 1
fi

if [[ ! -x "${VCPKG_ROOT}/vcpkg" ]]; then
  if [[ ! -d "${VCPKG_ROOT}/.git" ]]; then
    mkdir -p "$(dirname "${VCPKG_ROOT}")"
    git clone https://github.com/microsoft/vcpkg.git "${VCPKG_ROOT}"
  fi
  git -C "${VCPKG_ROOT}" fetch --depth 1 origin "${VCPKG_BASELINE}"
  git -C "${VCPKG_ROOT}" checkout --detach FETCH_HEAD
  "${VCPKG_ROOT}/bootstrap-vcpkg.sh" -disableMetrics
fi

# MXL CMake presets look for ~/vcpkg even when VCPKG_ROOT is elsewhere.
if [[ ! -e "${HOME}/vcpkg" && "${VCPKG_ROOT}" != "${HOME}/vcpkg" ]]; then
  ln -s "${VCPKG_ROOT}" "${HOME}/vcpkg"
fi

echo "==> Cloning ${MXL_REPO} @ ${MXL_REF}"
rm -rf "${WORK_DIR}"
git clone --depth 1 --branch "${MXL_REF}" "${MXL_REPO}" "${WORK_DIR}"

# Slim manifest: skip catch2 / cli11 / ada-url / pcapplusplus (tests and tools).
cat > "${WORK_DIR}/vcpkg.json" <<EOF
{
  "dependencies": [
    { "name": "stduuid", "features": ["system-gen", "gsl-span"] },
    "spdlog",
    "fmt",
    "picojson"
  ],
  "builtin-baseline": "${VCPKG_BASELINE}"
}
EOF

echo "==> Building libmxl (CMake Release + vcpkg)"
cmake -S "${WORK_DIR}" -B "${WORK_DIR}/build" \
  -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="${PREFIX}" \
  -DCMAKE_TOOLCHAIN_FILE="${VCPKG_ROOT}/scripts/buildsystems/vcpkg.cmake" \
  -DBUILD_DOCS=OFF \
  -DBUILD_TESTS=OFF \
  -DBUILD_TOOLS=OFF \
  -DBUILD_UTILS=OFF
cmake --build "${WORK_DIR}/build" -j "${JOBS}"
cmake --install "${WORK_DIR}/build"

LIBDIR="${PREFIX}/lib"
if [[ ! -e "${LIBDIR}/libmxl.so" && -e "${PREFIX}/lib64/libmxl.so" ]]; then
  LIBDIR="${PREFIX}/lib64"
fi
if [[ ! -e "${LIBDIR}/libmxl.so" ]]; then
  echo "error: libmxl.so was not installed under ${PREFIX}" >&2
  ls -la "${PREFIX}/lib" "${PREFIX}/lib64" 2>/dev/null || true
  exit 1
fi

export PKG_CONFIG_PATH="${LIBDIR}/pkgconfig:${PKG_CONFIG_PATH:-}"
export LD_LIBRARY_PATH="${LIBDIR}:${LD_LIBRARY_PATH:-}"

# gst-mxl-rs --features mxl/mxl-not-built compiles get_mxl_so_path() as
# $MXL_BUILD_DIR/lib/libmxl.so assembled at runtime. rust/mxl/build.rs sets
# MXL_BUILD_DIR to <repo>/build/Linux-Clang-Release, which is
# /tmp/mxl-sdk-build/... in Docker and does not exist in the runtime image.
# Bake the installed library path as a single string so mxlsink dlopens
# /usr/local/lib/libmxl.so (the copy we install into the runtime image).
python3 - "${WORK_DIR}/rust/mxl/src/config.rs" "${LIBDIR}/libmxl.so" <<'PY'
import re
import sys
from pathlib import Path

path = Path(sys.argv[1])
so_path = sys.argv[2]
text = path.read_text()
pat = re.compile(
    r'#\[cfg\(feature = "mxl-not-built"\)\]\s*'
    r'pub fn get_mxl_so_path\(\) -> std::path::PathBuf \{.*?\n\}',
    re.DOTALL,
)
new = (
    '#[cfg(feature = "mxl-not-built")]\n'
    'pub fn get_mxl_so_path() -> std::path::PathBuf {\n'
    f'    std::path::PathBuf::from("{so_path}")\n'
    '}'
)
if not pat.search(text):
    raise SystemExit(f"error: {path} mxl-not-built get_mxl_so_path() did not match")
path.write_text(pat.sub(new, text, count=1))
print(f"patched {path}: get_mxl_so_path -> {so_path}")
PY

echo "==> Building gst-mxl-rs plugin"
if [[ ! -d "${WORK_DIR}/rust/gst-mxl-rs" ]]; then
  echo "error: rust/gst-mxl-rs missing on ${MXL_REF}" >&2
  exit 1
fi
(
  cd "${WORK_DIR}/rust"
  cargo build --release -p gst-mxl-rs --features mxl/mxl-not-built
)

PLUGIN_SRC="${WORK_DIR}/rust/target/release/libgstmxl.so"
if [[ ! -f "${PLUGIN_SRC}" ]]; then
  echo "error: libgstmxl.so not found after cargo build" >&2
  find "${WORK_DIR}/rust/target" -name 'libgstmxl*' || true
  exit 1
fi
if grep -a -q 'Linux-Clang-Release/lib/libmxl.so' "${PLUGIN_SRC}"; then
  echo "error: libgstmxl.so still embeds the CMake-preset libmxl path" >&2
  exit 1
fi
if ! grep -a -q "${LIBDIR}/libmxl.so" "${PLUGIN_SRC}"; then
  echo "error: libgstmxl.so does not embed ${LIBDIR}/libmxl.so" >&2
  exit 1
fi

PLUGIN_DIR="${PREFIX}/lib/gstreamer-1.0"
mkdir -p "${PLUGIN_DIR}"
install -m0755 "${PLUGIN_SRC}" "${PLUGIN_DIR}/libgstmxl.so"

if command -v dpkg-architecture >/dev/null; then
  sys_plugin_dir="/usr/lib/$(dpkg-architecture -qDEB_HOST_MULTIARCH)/gstreamer-1.0"
  if [[ -d "${sys_plugin_dir}" ]]; then
    install -m0755 "${PLUGIN_SRC}" "${sys_plugin_dir}/libgstmxl.so"
  fi
fi

if [[ -n "${MXL_DIST_DIR:-}" ]]; then
  mkdir -p "${MXL_DIST_DIR}/lib" "${MXL_DIST_DIR}/gstreamer-1.0"
  cp -a "${LIBDIR}"/libmxl.so* "${MXL_DIST_DIR}/lib/"
  install -m0755 "${PLUGIN_SRC}" "${MXL_DIST_DIR}/gstreamer-1.0/libgstmxl.so"
fi

ldconfig || true
echo "==> Installed MXL SDK to ${PREFIX}"
echo "    export LD_LIBRARY_PATH=${LIBDIR}:\${LD_LIBRARY_PATH}"
echo "    export GST_PLUGIN_PATH=${PLUGIN_DIR}:\${GST_PLUGIN_PATH}"
