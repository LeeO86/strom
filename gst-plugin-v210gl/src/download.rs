//! Public `v210gldownload` bin: RGBA GLMemory → v210 sysmem.
//!
//! Readback uses stock `gldownload`, which already pipelines PBOs on
//! GStreamer 1.22+. That is required for 4–6× 1080p50.

use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer::subclass::prelude::*;
use std::sync::{Arc, OnceLock};

use crate::caps;
use crate::color;
use crate::properties::{self, ColorSettings};

#[derive(Default)]
pub struct V210GlDownload {
    settings: Arc<ColorSettings>,
    pack: glib::WeakRef<gst::Element>,
    unproxy: glib::WeakRef<gst::Element>,
}

#[glib::object_subclass]
impl ObjectSubclass for V210GlDownload {
    const NAME: &'static str = "GstV210GlDownload";
    type Type = super::V210GlDownload;
    type ParentType = gst::Bin;
}

impl ObjectImpl for V210GlDownload {
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
        if let Some(pack) = self.pack.upgrade() {
            pack.set_property_from_value(pspec.name(), value);
        }
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        properties::get_color_property(&self.settings, pspec)
    }

    fn constructed(&self) {
        self.parent_constructed();
        let bin = self.obj();
        if let Err(e) = build_download_bin(&bin, &self.settings, &self.pack, &self.unproxy) {
            gst::error!(CAT, obj = bin, "{e}");
        }
    }
}

impl GstObjectImpl for V210GlDownload {}

impl ElementImpl for V210GlDownload {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static META: OnceLock<gst::subclass::ElementMetadata> = OnceLock::new();
        Some(META.get_or_init(|| {
            gst::subclass::ElementMetadata::new(
                "v210 GL download",
                "Filter/Converter/Video",
                "Download RGBA GLMemory to v210 system memory via an RGB10A2 proxy texture. \
                 Uses gldownload for readback (PBO-capable). Mixer path is 8-bit RGBA.",
                "Strom contributors",
            )
        }))
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static TEMPLATES: OnceLock<Vec<gst::PadTemplate>> = OnceLock::new();
        TEMPLATES.get_or_init(|| {
            vec![
                gst::PadTemplate::new(
                    "sink",
                    gst::PadDirection::Sink,
                    gst::PadPresence::Always,
                    &caps::rgba_gl_caps(),
                )
                .unwrap(),
                gst::PadTemplate::new(
                    "src",
                    gst::PadDirection::Src,
                    gst::PadPresence::Always,
                    &caps::v210_caps(),
                )
                .unwrap(),
            ]
        })
    }
}

impl BinImpl for V210GlDownload {}

static CAT: std::sync::LazyLock<gst::DebugCategory> = std::sync::LazyLock::new(|| {
    gst::DebugCategory::new(
        "v210gldownload",
        gst::DebugColorFlags::empty(),
        Some("v210 GL download bin"),
    )
});

fn build_download_bin(
    bin: &super::V210GlDownload,
    settings: &Arc<ColorSettings>,
    pack_slot: &glib::WeakRef<gst::Element>,
    unproxy_slot: &glib::WeakRef<gst::Element>,
) -> Result<(), String> {
    let pack = gst::ElementFactory::make("v210glpack")
        .name("pack")
        .build()
        .map_err(|e| format!("v210glpack: {e}"))?;
    let download = gst::ElementFactory::make("gldownload")
        .name("download")
        .build()
        .map_err(|e| format!("gldownload: {e}"))?;
    let unproxy = gst::ElementFactory::make("v210glunproxy")
        .name("unproxy")
        .build()
        .map_err(|e| format!("v210glunproxy: {e}"))?;

    bin.add_many([&pack, &download, &unproxy])
        .map_err(|e| format!("add children: {e}"))?;
    gst::Element::link_many([&pack, &download, &unproxy])
        .map_err(|e| format!("link children: {e}"))?;

    let sink_pad = pack
        .static_pad("sink")
        .ok_or_else(|| "pack sink pad missing".to_string())?;
    let src_pad = unproxy
        .static_pad("src")
        .ok_or_else(|| "unproxy src pad missing".to_string())?;
    let sink_templ = bin
        .pad_template("sink")
        .ok_or_else(|| "sink pad template missing".to_string())?;
    let src_templ = bin
        .pad_template("src")
        .ok_or_else(|| "src pad template missing".to_string())?;
    let ghost_sink = gst::GhostPad::from_template_with_target(&sink_templ, &sink_pad)
        .map_err(|e| format!("ghost sink: {e}"))?;
    let ghost_src = gst::GhostPad::from_template_with_target(&src_templ, &src_pad)
        .map_err(|e| format!("ghost src: {e}"))?;
    ghost_sink.set_active(true).ok();
    ghost_src.set_active(true).ok();
    bin.add_pad(&ghost_sink)
        .map_err(|e| format!("add sink: {e}"))?;
    bin.add_pad(&ghost_src)
        .map_err(|e| format!("add src: {e}"))?;

    pack_slot.set(Some(&pack));
    unproxy_slot.set(Some(&unproxy));

    let pack_weak = pack.downgrade();
    let unproxy_weak = unproxy.downgrade();
    let settings = Arc::clone(settings);
    ghost_sink.add_probe(gst::PadProbeType::EVENT_DOWNSTREAM, move |_, info| {
        if let Some(event) = info.event() {
            if let gst::EventView::Caps(c) = event.view() {
                if let Some(s) = c.caps().structure(0) {
                    if let Ok(w) = s.get::<i32>("width") {
                        if let Some(pack) = pack_weak.upgrade() {
                            pack.set_property("video-width", w);
                        }
                        if let Some(unproxy) = unproxy_weak.upgrade() {
                            unproxy.set_property("video-width", w);
                        }
                    }
                    if let Ok(h) = s.get::<i32>("height") {
                        if let Some(pack) = pack_weak.upgrade() {
                            pack.set_property("video-height", h);
                        }
                    }
                    if settings.override_is_auto() {
                        if let Ok(colorimetry) = s.get::<&str>("colorimetry") {
                            if let Some(pack) = pack_weak.upgrade() {
                                pack.set_property(
                                    "colorimetry-override",
                                    color::matrix_from_colorimetry(colorimetry).as_str(),
                                );
                                pack.set_property(
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
