//! In-place caps rewrite: `RGB10A2_LE` (proxy width) → `v210` (real width).

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

#[derive(Default)]
pub struct V210GlUnproxy {
    original_width: AtomicI32,
}

#[glib::object_subclass]
impl ObjectSubclass for V210GlUnproxy {
    const NAME: &'static str = "GstV210GlUnproxy";
    type Type = super::V210GlUnproxy;
    type ParentType = BaseTransform;
}

impl ObjectImpl for V210GlUnproxy {
    fn properties() -> &'static [glib::ParamSpec] {
        static PROPS: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| {
            vec![glib::ParamSpecInt::builder("video-width")
                .nick("Video width")
                .blurb("Original v210 width")
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

impl GstObjectImpl for V210GlUnproxy {}

impl ElementImpl for V210GlUnproxy {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static META: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
            gst::subclass::ElementMetadata::new(
                "v210 GL unproxy caps",
                "Filter/Converter/Video",
                "Reinterpret RGB10A2_LE proxy memory as v210 (no copy)",
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
                    &caps::rgb10a2_caps(),
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
        });
        TEMPLATES.as_ref()
    }
}

impl BaseTransformImpl for V210GlUnproxy {
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
        let mut w = self.original_width.load(Ordering::Relaxed);
        if w <= 0 {
            if let Some(from_caps) = caps::original_width_from_caps(caps) {
                w = from_caps;
                self.original_width.store(w, Ordering::Relaxed);
            } else if let Some(from_pack) = caps::sibling_property_i32(
                self.obj().upcast_ref::<gst::Element>(),
                "pack",
                "video-width",
            ) {
                w = from_pack;
                self.original_width.store(w, Ordering::Relaxed);
            }
        }
        let out = match direction {
            gst::PadDirection::Sink => caps::rewrite_proxy_to_v210(caps, (w > 0).then_some(w))?,
            gst::PadDirection::Src => caps::rewrite_v210_to_proxy(caps)?,
            _ => return Some(caps.clone()),
        };
        Some(caps::intersect_or_copy(out, filter))
    }

    fn transform_ip(&self, buf: &mut gst::BufferRef) -> Result<gst::FlowSuccess, gst::FlowError> {
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
        Some(if format == "v210" {
            v210::frame_size(width, height)
        } else {
            width as usize * 4 * height as usize
        })
    }
}
