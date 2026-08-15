# T0 findings: MXL ↔ GPU v210 path

> Archived spike notes. Code is the source of truth — this may have drifted; read the code.

Checked against this tree and the host GStreamer (`gst-inspect-1.0`, plugin 1.24.2 / repo pin 1.22.12).

| Check | Result |
|---|---|
| `glupload` raw formats | v210 **absent**. `RGB10A2_LE` and `BGR10A2_LE` **present**. |
| `GL_UNSIGNED_INT_2_10_10_10_REV` mapping | GStreamer `RGB10A2_LE` (R = bits 0–9, G = 10–19, B = 20–29). Matches the v210 word layout. |
| `gstreamer-gl` 0.25 subclassing | `GLFilterImpl` / `GLBaseFilterImpl` / `render_to_target_with_shader` are available. Option A is viable. |
| `glvideomixerelement` pad templates | **RGBA only** (8-bit). Differing input sizes are a mixer feature; not re-tested here. |
| Mixer internal format | 8-bit RGBA. A 10-bit v210 round-trip through the mixer is **not** bit-exact. Documented on the MXL video blocks. |

Selected implementation: Option A (Rust elements in `gst-plugin-v210gl`), using `glupload`/`gldownload` for the proxy texture so readback stays on the stock PBO path.
