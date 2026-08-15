//! GLFilter: RGB10A2_LE GLMemory (proxy width) → RGBA GLMemory (real width).

use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer::subclass::prelude::*;
use gstreamer_base as gst_base;
use gstreamer_gl::subclass::prelude::*;
use gstreamer_gl::{GLFilter, GLMemory, GLShader};
use std::sync::{Mutex, OnceLock};

use crate::caps;
use crate::glfilter;
use crate::properties::{self, ColorSettings};
use crate::shaders;
use crate::v210;

#[derive(Default)]
pub struct V210GlUnpack {
    pub settings: ColorSettings,
    shader: Mutex<Option<GLShader>>,
}

#[glib::object_subclass]
impl ObjectSubclass for V210GlUnpack {
    const NAME: &'static str = "GstV210GlUnpack";
    type Type = super::V210GlUnpack;
    type ParentType = GLFilter;
}

impl ObjectImpl for V210GlUnpack {
    fn properties() -> &'static [glib::ParamSpec] {
        static PROPS: OnceLock<Vec<glib::ParamSpec>> = OnceLock::new();
        PROPS.get_or_init(properties::color_properties)
    }

    fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
        properties::set_color_property(&self.settings, value, pspec);
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        properties::get_color_property(&self.settings, pspec)
    }
}

impl GstObjectImpl for V210GlUnpack {}

impl ElementImpl for V210GlUnpack {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static META: OnceLock<gst::subclass::ElementMetadata> = OnceLock::new();
        Some(META.get_or_init(|| {
            gst::subclass::ElementMetadata::new(
                "v210 GL unpack",
                "Filter/Converter/Video",
                "Unpack RGB10A2 v210 proxy texture to RGBA GLMemory",
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
                    &caps::rgb10a2_gl_caps(),
                )
                .unwrap(),
                gst::PadTemplate::new(
                    "src",
                    gst::PadDirection::Src,
                    gst::PadPresence::Always,
                    &caps::rgba_gl_caps(),
                )
                .unwrap(),
            ]
        })
    }
}

impl BaseTransformImpl for V210GlUnpack {
    const MODE: gst_base::subclass::BaseTransformMode =
        gst_base::subclass::BaseTransformMode::NeverInPlace;
    const PASSTHROUGH_ON_SAME_CAPS: bool = false;
    const TRANSFORM_IP_ON_PASSTHROUGH: bool = false;
}

impl GLBaseFilterImpl for V210GlUnpack {
    fn gl_stop(&self) {
        if let Ok(mut shader) = self.shader.lock() {
            *shader = None;
        }
        self.parent_gl_stop();
    }
}

impl GLFilterImpl for V210GlUnpack {
    const MODE: gstreamer_gl::subclass::GLFilterMode =
        gstreamer_gl::subclass::GLFilterMode::Texture;
    const ADD_RGBA_PAD_TEMPLATES: bool = false;

    fn transform_internal_caps(
        &self,
        direction: gst::PadDirection,
        caps: &gst::Caps,
        filter_caps: Option<&gst::Caps>,
    ) -> Option<gst::Caps> {
        let s = caps.structure(0)?;
        let (prop_w, oh) = self.settings.video_size();
        let ow = resolve_original_width(
            &self.settings,
            self.obj().upcast_ref::<gst::Element>(),
            caps,
            prop_w,
        );
        let mut out = match direction {
            gst::PadDirection::Sink => {
                let mut c = caps::copy_video_fields(s, caps::rgba_gl_caps());
                if let Some(st) = c.make_mut().structure_mut(0) {
                    if ow > 0 {
                        st.set("width", ow as i32);
                    }
                    if oh > 0 {
                        st.set("height", oh as i32);
                    }
                    st.set("format", "RGBA");
                }
                c
            }
            gst::PadDirection::Src => {
                let mut c = caps::copy_video_fields(s, caps::rgb10a2_gl_caps());
                if let Some(st) = c.make_mut().structure_mut(0) {
                    let w = if ow > 0 {
                        v210::proxy_width(ow) as i32
                    } else {
                        s.get::<i32>("width").unwrap_or(0)
                    };
                    if w > 0 {
                        st.set("width", w);
                    }
                    st.set("format", "RGB10A2_LE");
                }
                c
            }
            _ => caps.clone(),
        };
        if let Some(f) = filter_caps {
            out = out.intersect_with_mode(f, gst::CapsIntersectMode::First);
        }
        Some(out)
    }

    fn filter_texture(
        &self,
        input: &GLMemory,
        output: &GLMemory,
    ) -> Result<(), gst::LoggableError> {
        let filter = self.obj();
        let filter = filter.upcast_ref::<gstreamer_gl::GLFilter>();
        let context = glfilter::context_from_filter(filter)
            .ok_or_else(|| gst::loggable_error!(gst::CAT_DEFAULT, "no GL context"))?;

        let mut shader_guard = self
            .shader
            .lock()
            .map_err(|_| gst::loggable_error!(gst::CAT_DEFAULT, "shader mutex poisoned"))?;
        if shader_guard.is_none() {
            *shader_guard = Some(
                glfilter::compile_shader(&context, shaders::UNPACK_FRAGMENT)
                    .map_err(|e| gst::loggable_error!(gst::CAT_DEFAULT, "{e}"))?,
            );
        }
        let shader = shader_guard.as_ref().unwrap();
        shader.use_();
        let (ow, oh) = self.settings.video_size();
        let pw = v210::proxy_width(ow) as f32;
        shader.set_uniform_1f("u_proxy_width", pw);
        shader.set_uniform_1f("u_out_width", ow as f32);
        shader.set_uniform_1f("u_out_height", oh as f32);
        glfilter::apply_matrix_uniforms(shader, &self.settings);
        glfilter::render_with_shader(filter, input, output, shader);
        Ok(())
    }
}

fn resolve_original_width(
    settings: &crate::properties::ColorSettings,
    elem: &gst::Element,
    caps: &gst::Caps,
    prop_w: u32,
) -> u32 {
    if prop_w > 0 {
        return prop_w;
    }
    if let Some(w) = caps::original_width_from_caps(caps) {
        settings
            .video_width
            .store(w, std::sync::atomic::Ordering::Relaxed);
        return w as u32;
    }
    if let Some(w) = caps::sibling_property_i32(elem, "proxy", "video-width") {
        settings
            .video_width
            .store(w, std::sync::atomic::Ordering::Relaxed);
        return w as u32;
    }
    0
}
