//! GLSL fragments for v210 unpack/pack.
//!
//! Written in the same GLES2-style dialect Strom's `glshader` FX uses
//! (`texture2D`, `gl_FragColor`, no `#version`). Sampling uses texel-center
//! UVs so a nearest-filtered `RGB10A2` proxy texture is bit-stable.

/// Fragment that unpacks an RGB10A2 proxy texture to RGBA.
///
/// Uniforms: `tex`, `u_proxy_width`, `u_out_width`, `u_out_height`,
/// `u_full_range`, `u_kr`, `u_kb`.
pub const UNPACK_FRAGMENT: &str = r#"
#ifdef GL_ES
precision highp float;
#endif
varying vec2 v_texcoord;
uniform sampler2D tex;
uniform float u_proxy_width;
uniform float u_out_width;
uniform float u_out_height;
uniform float u_full_range;
uniform float u_kr;
uniform float u_kb;

int field10(float n) {
    return int(floor(n * 1023.0 + 0.5));
}

void main() {
    float xf = v_texcoord.x * u_out_width;
    int pixel = int(floor(xf));
    int group = pixel / 6;
    int phase = pixel - group * 6;

    float y0 = (float(group * 4) + 0.5) / u_proxy_width;
    float row = (floor(v_texcoord.y * u_out_height) + 0.5) / u_out_height;
    vec4 w0 = texture2D(tex, vec2(y0, row));
    vec4 w1 = texture2D(tex, vec2(y0 + 1.0 / u_proxy_width, row));
    vec4 w2 = texture2D(tex, vec2(y0 + 2.0 / u_proxy_width, row));
    vec4 w3 = texture2D(tex, vec2(y0 + 3.0 / u_proxy_width, row));

    int cb0 = field10(w0.r);
    int y_0 = field10(w0.g);
    int cr0 = field10(w0.b);
    int y_1 = field10(w1.r);
    int cb2 = field10(w1.g);
    int y_2 = field10(w1.b);
    int cr2 = field10(w2.r);
    int y_3 = field10(w2.g);
    int cb4 = field10(w2.b);
    int y_4 = field10(w3.r);
    int cr4 = field10(w3.g);
    int y_5 = field10(w3.b);

    int yv;
    int cb;
    int cr;
    if (phase == 0) { yv = y_0; cb = cb0; cr = cr0; }
    else if (phase == 1) { yv = y_1; cb = cb0; cr = cr0; }
    else if (phase == 2) { yv = y_2; cb = cb2; cr = cr2; }
    else if (phase == 3) { yv = y_3; cb = cb2; cr = cr2; }
    else if (phase == 4) { yv = y_4; cb = cb4; cr = cr4; }
    else { yv = y_5; cb = cb4; cr = cr4; }

    float yn;
    float cbn;
    float crn;
    if (u_full_range > 0.5) {
        yn = float(yv) / 1023.0;
        cbn = (float(cb) - 512.0) / 512.0;
        crn = (float(cr) - 512.0) / 512.0;
    } else {
        yn = (float(yv) - 64.0) / 876.0;
        cbn = (float(cb) - 512.0) / 896.0;
        crn = (float(cr) - 512.0) / 896.0;
    }

    float cb_scale = 2.0 * (1.0 - u_kb);
    float cr_scale = 2.0 * (1.0 - u_kr);
    float kg = 1.0 - u_kr - u_kb;
    float r = yn + crn * cr_scale;
    float b = yn + cbn * cb_scale;
    float g = yn - (u_kb * cb_scale / kg) * cbn - (u_kr * cr_scale / kg) * crn;
    gl_FragColor = vec4(clamp(r, 0.0, 1.0), clamp(g, 0.0, 1.0), clamp(b, 0.0, 1.0), 1.0);
}
"#;

/// Fragment that packs RGBA into an RGB10A2 proxy texture (v210 words).
///
/// Uniforms: `tex`, `u_proxy_width`, `u_in_width`, `u_in_height`,
/// `u_full_range`, `u_kr`, `u_kb`.
pub const PACK_FRAGMENT: &str = r#"
#ifdef GL_ES
precision highp float;
#endif
varying vec2 v_texcoord;
uniform sampler2D tex;
uniform float u_proxy_width;
uniform float u_in_width;
uniform float u_in_height;
uniform float u_full_range;
uniform float u_kr;
uniform float u_kb;

int clamp10(float v) {
    return int(clamp(floor(v + 0.5), 0.0, 1023.0));
}

vec3 rgb_to_ycbcr(vec3 rgb) {
    float yn = u_kr * rgb.r + (1.0 - u_kr - u_kb) * rgb.g + u_kb * rgb.b;
    float cb_scale = 2.0 * (1.0 - u_kb);
    float cr_scale = 2.0 * (1.0 - u_kr);
    float cbn = (rgb.b - yn) / cb_scale;
    float crn = (rgb.r - yn) / cr_scale;
    float y;
    float cb;
    float cr;
    if (u_full_range > 0.5) {
        y = yn * 1023.0;
        cb = cbn * 512.0 + 512.0;
        cr = crn * 512.0 + 512.0;
    } else {
        y = yn * 876.0 + 64.0;
        cb = cbn * 896.0 + 512.0;
        cr = crn * 896.0 + 512.0;
    }
    return vec3(float(clamp10(y)), float(clamp10(cb)), float(clamp10(cr)));
}

void main() {
    float word_f = floor(v_texcoord.x * u_proxy_width);
    int word = int(word_f);
    int group = word / 4;
    int wi = word - group * 4;
    float row = (floor(v_texcoord.y * u_in_height) + 0.5) / u_in_height;

    vec3 p0 = rgb_to_ycbcr(texture2D(tex, vec2((float(group * 6) + 0.5) / u_in_width, row)).rgb);
    vec3 p1 = rgb_to_ycbcr(texture2D(tex, vec2((float(group * 6) + 1.5) / u_in_width, row)).rgb);
    vec3 p2 = rgb_to_ycbcr(texture2D(tex, vec2((float(group * 6) + 2.5) / u_in_width, row)).rgb);
    vec3 p3 = rgb_to_ycbcr(texture2D(tex, vec2((float(group * 6) + 3.5) / u_in_width, row)).rgb);
    vec3 p4 = rgb_to_ycbcr(texture2D(tex, vec2((float(group * 6) + 4.5) / u_in_width, row)).rgb);
    vec3 p5 = rgb_to_ycbcr(texture2D(tex, vec2((float(group * 6) + 5.5) / u_in_width, row)).rgb);

    vec3 outc;
    if (wi == 0) {
        outc = vec3(p0.y, p0.x, p0.z);
    } else if (wi == 1) {
        outc = vec3(p1.x, p2.y, p2.x);
    } else if (wi == 2) {
        outc = vec3(p2.z, p3.x, p4.y);
    } else {
        outc = vec3(p4.x, p4.z, p5.x);
    }
    gl_FragColor = vec4(outc / 1023.0, 0.0);
}
"#;
