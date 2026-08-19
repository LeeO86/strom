#!/usr/bin/env bash
# Verify libmxl + mxlsrc/mxlsink are visible to GStreamer.
set -euo pipefail

errors=0

if ! ldconfig -p 2>/dev/null | grep -q libmxl; then
  if [[ ! -f /usr/local/lib/libmxl.so && ! -f /usr/lib/libmxl.so ]]; then
    echo "FAIL: libmxl.so not found"
    errors=$((errors + 1))
  else
    echo "OK: libmxl.so present on disk"
  fi
else
  echo "OK: libmxl.so in ldconfig"
fi

if ! command -v gst-inspect-1.0 >/dev/null; then
  echo "FAIL: gst-inspect-1.0 not installed"
  exit 1
fi

if gst-inspect-1.0 mxlsrc >/dev/null 2>&1; then
  echo "OK: mxlsrc"
else
  echo "FAIL: mxlsrc not registered (set GST_PLUGIN_PATH to libgstmxl.so)"
  errors=$((errors + 1))
fi

if gst-inspect-1.0 mxlsink >/dev/null 2>&1; then
  echo "OK: mxlsink"
else
  echo "FAIL: mxlsink not registered"
  errors=$((errors + 1))
fi

plugin=""
for candidate in \
  "/usr/lib/$(uname -m)-linux-gnu/gstreamer-1.0/libgstmxl.so" \
  /usr/local/lib/gstreamer-1.0/libgstmxl.so
do
  if [[ -f "${candidate}" ]]; then
    plugin="${candidate}"
    break
  fi
done
if [[ -n "${plugin}" ]]; then
  if grep -a -q 'Linux-Clang-Release/lib/libmxl.so' "${plugin}"; then
    echo "FAIL: ${plugin} embeds the MXL build-tree libmxl path (mxlsink will dlopen /tmp/.../libmxl.so)"
    errors=$((errors + 1))
  else
    echo "OK: ${plugin} does not embed the MXL build-tree path"
  fi
fi

if gst-inspect-1.0 v210glupload >/dev/null 2>&1; then
  echo "OK: v210glupload (Strom plugin already loaded)"
else
  echo "INFO: v210glupload not in gst-inspect — it is registered by the Strom binary"
fi

exit "${errors}"
