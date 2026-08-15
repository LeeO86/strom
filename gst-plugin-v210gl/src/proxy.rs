//! In-place caps rewrite: `video/x-raw,format=v210` → `RGB10A2_LE` at proxy width.
//!
//! The buffer bytes are not touched. `GstVideoMeta` is dropped so `glupload`
//! uses the RGB10A2 layout implied by the new caps. Byte count matches
//! (`proxy_width * 4 == v210_stride`).

use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer::subclass::prelude::*;
use gstreamer_base as gst_base;
use gstreamer_base::prelude::*;
use gstreamer_base::subclass::prelude::*;
use gstreamer_base::BaseTransform;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::LazyLock;

use crate::caps;
use crate::v210;

static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
    gst::DebugCategory::new(
        "v210glproxy",
        gst::DebugColorFlags::empty(),
        Some("v210 to RGB10A2 proxy caps rewrite"),
    )
});

#[derive(Default)]
pub struct V210GlProxy {
    original_width: AtomicI32,
}

#[glib::object_subclass]
impl ObjectSubclass for V210GlProxy {
    const NAME: &'static str = "GstV210GlProxy";
    type Type = super::V210GlProxy;
    type ParentType = BaseTransform;
}

impl ObjectImpl for V210GlProxy {
    fn properties() -> &'static [glib::ParamSpec] {
        static PROPS: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| {
            vec![glib::ParamSpecInt::builder("video-width")
                .nick("Video width")
                .blurb("Original v210 width in pixels")
                .minimum(0)
                .maximum(i32::MAX)
                .default_value(0)
                .build()]
        });
        PROPS.as_ref()
    }

    fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
        if pspec.name() == "video-width" {
            if let Ok(v) = value.get::<i32>() {
                self.original_width.store(v, Ordering::Relaxed);
            }
        }
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        if pspec.name() == "video-width" {
            return self.original_width.load(Ordering::Relaxed).to_value();
        }
        "".to_value()
    }

    fn constructed(&self) {
        self.parent_constructed();
        self.obj().set_in_place(true);
        self.obj().set_passthrough(false);
    }
}

impl GstObjectImpl for V210GlProxy {}

impl ElementImpl for V210GlProxy {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static META: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
            gst::subclass::ElementMetadata::new(
                "v210 GL proxy caps",
                "Filter/Converter/Video",
                "Reinterpret v210 system memory as RGB10A2_LE of proxy width (no copy)",
                "Strom contributors",
            )
        });
        Some(&*META)
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static TEMPLATES: LazyLock<Vec<gst::PadTemplate>> = LazyLock::new(|| {
            vec![
                gst::PadTemplate::new(
                    "sink",
                    gst::PadDirection::Sink,
                    gst::PadPresence::Always,
                    &caps::v210_caps(),
                )
                .unwrap(),
                gst::PadTemplate::new(
                    "src",
                    gst::PadDirection::Src,
                    gst::PadPresence::Always,
                    &caps::rgb10a2_caps(),
                )
                .unwrap(),
            ]
        });
        TEMPLATES.as_ref()
    }
}

impl BaseTransformImpl for V210GlProxy {
    const MODE: gst_base::subclass::BaseTransformMode =
        gst_base::subclass::BaseTransformMode::AlwaysInPlace;
    const PASSTHROUGH_ON_SAME_CAPS: bool = false;
    const TRANSFORM_IP_ON_PASSTHROUGH: bool = false;

    fn transform_caps(
        &self,
        direction: gst::PadDirection,
        caps: &gst::Caps,
        filter: Option<&gst::Caps>,
    ) -> Option<gst::Caps> {
        let out = match direction {
            gst::PadDirection::Sink => {
                if let Some(s) = caps.structure(0) {
                    if let Ok(w) = s.get::<i32>("width") {
                        if w > 0 {
                            self.original_width.store(w, Ordering::Relaxed);
                        }
                    }
                }
                caps::rewrite_v210_to_proxy(caps)?
            }
            gst::PadDirection::Src => {
                let w = self.original_width.load(Ordering::Relaxed);
                caps::rewrite_proxy_to_v210(caps, (w > 0).then_some(w))?
            }
            _ => return Some(caps.clone()),
        };
        Some(caps::intersect_or_copy(out, filter))
    }

    fn set_caps(&self, incaps: &gst::Caps, _outcaps: &gst::Caps) -> Result<(), gst::LoggableError> {
        if let Some(s) = incaps.structure(0) {
            if let Ok(mode) = s.get::<&str>("interlace-mode") {
                if mode != "progressive" {
                    return Err(gst::loggable_error!(
                        CAT,
                        "interlaced v210 is not supported (got interlace-mode={mode})"
                    ));
                }
            }
            if let Ok(w) = s.get::<i32>("width") {
                self.original_width.store(w, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    fn transform_ip(&self, buf: &mut gst::BufferRef) -> Result<gst::FlowSuccess, gst::FlowError> {
        // Drop VideoMeta so downstream uses RGB10A2 geometry from the caps.
        while let Some(meta) = buf.meta_mut::<gstreamer_video::VideoMeta>() {
            meta.remove().ok();
        }
        Ok(gst::FlowSuccess::Ok)
    }

    fn unit_size(&self, caps: &gst::Caps) -> Option<usize> {
        let s = caps.structure(0)?;
        let width = s.get::<i32>("width").ok()? as u32;
        let height = s.get::<i32>("height").ok()? as u32;
        let format = s.get::<&str>("format").ok()?;
        let size = if format == "v210" {
            v210::frame_size(width, height)
        } else {
            width as usize * 4 * height as usize
        };
        Some(size)
    }
}

impl crate::V210GlProxy {
    pub fn original_width(&self) -> i32 {
        self.imp().original_width.load(Ordering::Relaxed)
    }
}
