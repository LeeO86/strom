//! Public `v210glupload` bin: v210 sysmem → RGBA GLMemory.

use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer::subclass::prelude::*;
use std::sync::{Arc, OnceLock};

use crate::color;
use crate::properties::{self, ColorSettings};

#[derive(Default)]
pub struct V210GlUpload {
    settings: Arc<ColorSettings>,
    unpack: glib::WeakRef<gst::Element>,
}

#[glib::object_subclass]
impl ObjectSubclass for V210GlUpload {
    const NAME: &'static str = "GstV210GlUpload";
    type Type = super::V210GlUpload;
    type ParentType = gst::Bin;
}

impl ObjectImpl for V210GlUpload {
    fn properties() -> &'static [glib::ParamSpec] {
        static PROPS: OnceLock<Vec<glib::ParamSpec>> = OnceLock::new();
        PROPS.get_or_init(|| {
            properties::color_properties()
                .into_iter()
                .filter(|p| p.name() != "video-width" && p.name() != "video-height")
                .collect()
        })
    }

    fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
        properties::set_color_property(&self.settings, value, pspec);
        if let Some(unpack) = self.unpack.upgrade() {
            unpack.set_property_from_value(pspec.name(), value);
        }
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        properties::get_color_property(&self.settings, pspec)
    }

    fn constructed(&self) {
        self.parent_constructed();
        let bin = self.obj();
        if let Err(e) = build_upload_bin(&bin, &self.settings, &self.unpack) {
            gst::error!(CAT, obj = bin, "{e}");
        }
    }
}

impl GstObjectImpl for V210GlUpload {}

impl ElementImpl for V210GlUpload {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static META: OnceLock<gst::subclass::ElementMetadata> = OnceLock::new();
        Some(META.get_or_init(|| {
            gst::subclass::ElementMetadata::new(
                "v210 GL upload",
                "Filter/Converter/Video",
                "Upload v210 system memory to RGBA GLMemory via an RGB10A2 proxy texture. \
                 The vision mixer path is 8-bit RGBA — a 10-bit source is not bit-exact after mix.",
                "Strom contributors",
            )
        }))
    }
}

impl BinImpl for V210GlUpload {}

static CAT: std::sync::LazyLock<gst::DebugCategory> = std::sync::LazyLock::new(|| {
    gst::DebugCategory::new(
        "v210glupload",
        gst::DebugColorFlags::empty(),
        Some("v210 GL upload bin"),
    )
});

fn build_upload_bin(
    bin: &super::V210GlUpload,
    settings: &Arc<ColorSettings>,
    unpack_slot: &glib::WeakRef<gst::Element>,
) -> Result<(), String> {
    let proxy = gst::ElementFactory::make("v210glproxy")
        .name("proxy")
        .build()
        .map_err(|e| format!("v210glproxy: {e}"))?;
    let upload = gst::ElementFactory::make("glupload")
        .name("upload")
        .build()
        .map_err(|e| format!("glupload: {e}"))?;
    let unpack = gst::ElementFactory::make("v210glunpack")
        .name("unpack")
        .build()
        .map_err(|e| format!("v210glunpack: {e}"))?;

    bin.add_many([&proxy, &upload, &unpack])
        .map_err(|e| format!("add children: {e}"))?;
    gst::Element::link_many([&proxy, &upload, &unpack])
        .map_err(|e| format!("link children: {e}"))?;

    let sink_pad = proxy
        .static_pad("sink")
        .ok_or_else(|| "proxy sink pad missing".to_string())?;
    let src_pad = unpack
        .static_pad("src")
        .ok_or_else(|| "unpack src pad missing".to_string())?;
    // Do not put Always pad templates on this bin — they clash with ghost pads
    // and leave the element with no sink/src (GStreamer then warns at dispose).
    let ghost_sink = gst::GhostPad::builder_with_target(&sink_pad)
        .map_err(|e| format!("ghost sink: {e}"))?
        .name("sink")
        .build();
    let ghost_src = gst::GhostPad::builder_with_target(&src_pad)
        .map_err(|e| format!("ghost src: {e}"))?
        .name("src")
        .build();
    ghost_sink.set_active(true).ok();
    ghost_src.set_active(true).ok();
    bin.add_pad(&ghost_sink)
        .map_err(|e| format!("add sink: {e}"))?;
    bin.add_pad(&ghost_src)
        .map_err(|e| format!("add src: {e}"))?;

    unpack_slot.set(Some(&unpack));

    let unpack_weak = unpack.downgrade();
    let settings = Arc::clone(settings);
    ghost_sink.add_probe(gst::PadProbeType::EVENT_DOWNSTREAM, move |_, info| {
        if let Some(event) = info.event() {
            if let gst::EventView::Caps(c) = event.view() {
                if let Some(s) = c.caps().structure(0) {
                    if let Some(unpack) = unpack_weak.upgrade() {
                        if let Ok(w) = s.get::<i32>("width") {
                            unpack.set_property("video-width", w);
                        }
                        if let Ok(h) = s.get::<i32>("height") {
                            unpack.set_property("video-height", h);
                        }
                        if settings.override_is_auto() {
                            if let Ok(colorimetry) = s.get::<&str>("colorimetry") {
                                unpack.set_property(
                                    "colorimetry-override",
                                    color::matrix_from_colorimetry(colorimetry).as_str(),
                                );
                                unpack.set_property(
                                    "full-range",
                                    color::full_range_from_colorimetry(colorimetry),
                                );
                            }
                        }
                    }
                }
            }
        }
        gst::PadProbeReturn::Ok
    });
    Ok(())
}
