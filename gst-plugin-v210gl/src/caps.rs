//! Caps helpers for the v210 ↔ RGB10A2 proxy rewrite.

use gstreamer as gst;

use crate::v210;

pub fn v210_caps() -> gst::Caps {
    gst::Caps::builder("video/x-raw")
        .field("format", "v210")
        .field("interlace-mode", "progressive")
        .build()
}

pub fn rgb10a2_caps() -> gst::Caps {
    gst::Caps::builder("video/x-raw")
        .field("format", "RGB10A2_LE")
        .field("interlace-mode", "progressive")
        .build()
}

pub fn rgba_gl_caps() -> gst::Caps {
    gst::Caps::builder("video/x-raw")
        .features(["memory:GLMemory"])
        .field("format", "RGBA")
        .field("interlace-mode", "progressive")
        .build()
}

pub fn rgb10a2_gl_caps() -> gst::Caps {
    gst::Caps::builder("video/x-raw")
        .features(["memory:GLMemory"])
        .field("format", "RGB10A2_LE")
        .field("interlace-mode", "progressive")
        .build()
}

/// Copy width/height/framerate/pixel-aspect-ratio/colorimetry from `from` onto `template`.
pub fn copy_video_fields(from: &gst::StructureRef, template: gst::Caps) -> gst::Caps {
    let mut caps = template;
    {
        let caps = caps.make_mut();
        if let Some(out) = caps.structure_mut(0) {
            if let Ok(w) = from.get::<i32>("width") {
                out.set("width", w);
            }
            if let Ok(h) = from.get::<i32>("height") {
                out.set("height", h);
            }
            if let Ok(fr) = from.get::<gst::Fraction>("framerate") {
                out.set("framerate", fr);
            }
            if let Ok(par) = from.get::<gst::Fraction>("pixel-aspect-ratio") {
                out.set("pixel-aspect-ratio", par);
            }
            if let Ok(c) = from.get::<&str>("colorimetry") {
                out.set("colorimetry", c);
            }
        }
    }
    caps
}

fn field_contains_string(s: &gst::StructureRef, field: &str, needle: &str) -> bool {
    if !s.has_field(field) {
        return false;
    }
    if let Ok(v) = s.get::<&str>(field) {
        return v == needle;
    }
    if let Ok(list) = s.get::<gst::List>(field) {
        return list
            .iter()
            .any(|val| val.get::<&str>().ok() == Some(needle));
    }
    if let Ok(arr) = s.get::<gst::Array>(field) {
        return arr.iter().any(|val| val.get::<&str>().ok() == Some(needle));
    }
    false
}

fn is_progressive(s: &gst::StructureRef) -> bool {
    !s.has_field("interlace-mode") || field_contains_string(s, "interlace-mode", "progressive")
}

pub fn rewrite_v210_to_proxy(caps: &gst::Caps) -> Option<gst::Caps> {
    let mut out = gst::Caps::new_empty();
    for s in caps.iter() {
        if s.name() != "video/x-raw" {
            continue;
        }
        if !field_contains_string(s, "format", "v210") {
            continue;
        }
        if !is_progressive(s) {
            continue;
        }
        let mut converted = copy_video_fields(s, rgb10a2_caps());
        if let Some(st) = converted.make_mut().structure_mut(0) {
            if let Ok(width) = s.get::<i32>("width") {
                st.set("width", v210::proxy_width(width as u32) as i32);
                st.set("original-width", width);
            }
            st.set("format", "RGB10A2_LE");
            st.set("interlace-mode", "progressive");
        }
        out.make_mut().append(converted);
    }
    (!out.is_empty()).then_some(out)
}

pub fn rewrite_proxy_to_v210(caps: &gst::Caps, original_width: Option<i32>) -> Option<gst::Caps> {
    let mut out = gst::Caps::new_empty();
    for s in caps.iter() {
        if s.name() != "video/x-raw" {
            continue;
        }
        if !field_contains_string(s, "format", "RGB10A2_LE") {
            continue;
        }
        if !is_progressive(s) {
            continue;
        }
        let width = original_width
            .or_else(|| original_width_from_structure(s))
            .or_else(|| s.get::<i32>("width").ok())
            .unwrap_or(0);
        let mut converted = copy_video_fields(s, v210_caps());
        if let Some(st) = converted.make_mut().structure_mut(0) {
            if width > 0 {
                st.set("width", width);
            } else {
                st.remove_field("width");
            }
            st.set("format", "v210");
            st.set("interlace-mode", "progressive");
        }
        out.make_mut().append(converted);
    }
    (!out.is_empty()).then_some(out)
}

pub fn original_width_from_caps(caps: &gst::Caps) -> Option<i32> {
    original_width_from_structure(caps.structure(0)?)
}

fn original_width_from_structure(s: &gst::StructureRef) -> Option<i32> {
    s.get::<i32>("original-width").ok().filter(|w| *w > 0)
}

/// Read a positive i32 property from a sibling element in the parent bin.
pub fn sibling_property_i32(elem: &gst::Element, name: &str, prop: &str) -> Option<i32> {
    use gst::prelude::*;
    let parent = elem.parent()?.downcast::<gst::Bin>().ok()?;
    let sib = parent.by_name(name)?;
    sib.find_property(prop)?;
    let v = sib.property::<i32>(prop);
    (v > 0).then_some(v)
}

pub fn intersect_or_copy(caps: gst::Caps, filter: Option<&gst::Caps>) -> gst::Caps {
    match filter {
        Some(f) => caps.intersect_with_mode(f, gst::CapsIntersectMode::First),
        None => caps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_rewrite_keeps_original_width() {
        let _ = gst::init();
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "v210")
            .field("width", 1920i32)
            .field("height", 1080i32)
            .field("interlace-mode", "progressive")
            .build();
        let out = rewrite_v210_to_proxy(&caps).unwrap();
        let s = out.structure(0).unwrap();
        assert_eq!(s.get::<&str>("format").unwrap(), "RGB10A2_LE");
        assert_eq!(s.get::<i32>("width").unwrap(), 1280);
        assert_eq!(s.get::<i32>("original-width").unwrap(), 1920);
        assert_eq!(s.get::<i32>("height").unwrap(), 1080);
    }

    #[test]
    fn interlaced_v210_is_rejected() {
        let _ = gst::init();
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "v210")
            .field("width", 1920i32)
            .field("height", 1080i32)
            .field("interlace-mode", "interleaved")
            .build();
        assert!(rewrite_v210_to_proxy(&caps).is_none());
    }

    #[test]
    fn unproxy_uses_original_width_field() {
        let _ = gst::init();
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "RGB10A2_LE")
            .field("width", 1280i32)
            .field("height", 1080i32)
            .field("original-width", 1920i32)
            .build();
        let out = rewrite_proxy_to_v210(&caps, None).unwrap();
        let s = out.structure(0).unwrap();
        assert_eq!(s.get::<&str>("format").unwrap(), "v210");
        assert_eq!(s.get::<i32>("width").unwrap(), 1920);
    }

    #[test]
    fn proxy_rewrite_skips_leading_dmabuf_structure() {
        let _ = gst::init();
        let mut caps = gst::Caps::builder("video/x-raw")
            .field("format", "DMA_DRM")
            .build();
        caps.make_mut().append(
            gst::Caps::builder("video/x-raw")
                .field("format", gst::List::new(["RGBA", "RGB10A2_LE", "NV12"]))
                .build(),
        );
        let out = rewrite_proxy_to_v210(&caps, None).expect("RGB10A2 structure should match");
        assert_eq!(
            out.structure(0).unwrap().get::<&str>("format").unwrap(),
            "v210"
        );
    }
}
