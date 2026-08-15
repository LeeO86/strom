//! GStreamer plugin: GPU v210 ↔ RGBA conversion via an RGB10A2 proxy texture.

use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;

mod caps;
mod color;
mod download;
mod glfilter;
mod pack;
mod properties;
mod proxy;
mod shaders;
mod unpack;
mod unproxy;
mod upload;
pub mod v210;

glib::wrapper! {
    pub struct V210GlProxy(ObjectSubclass<proxy::V210GlProxy>) @extends gstreamer_base::BaseTransform, gst::Element, gst::Object;
}
glib::wrapper! {
    pub struct V210GlUnproxy(ObjectSubclass<unproxy::V210GlUnproxy>) @extends gstreamer_base::BaseTransform, gst::Element, gst::Object;
}
glib::wrapper! {
    pub struct V210GlUnpack(ObjectSubclass<unpack::V210GlUnpack>) @extends gstreamer_gl::GLFilter, gstreamer_gl::GLBaseFilter, gstreamer_base::BaseTransform, gst::Element, gst::Object;
}
glib::wrapper! {
    pub struct V210GlPack(ObjectSubclass<pack::V210GlPack>) @extends gstreamer_gl::GLFilter, gstreamer_gl::GLBaseFilter, gstreamer_base::BaseTransform, gst::Element, gst::Object;
}
glib::wrapper! {
    pub struct V210GlUpload(ObjectSubclass<upload::V210GlUpload>) @extends gst::Bin, gst::Element, gst::Object;
}
glib::wrapper! {
    pub struct V210GlDownload(ObjectSubclass<download::V210GlDownload>) @extends gst::Bin, gst::Element, gst::Object;
}

fn plugin_init(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    V210GlProxy::register(plugin)?;
    V210GlUnproxy::register(plugin)?;
    V210GlUnpack::register(plugin)?;
    V210GlPack::register(plugin)?;
    V210GlUpload::register(plugin)?;
    V210GlDownload::register(plugin)?;
    Ok(())
}

gst::plugin_define!(
    v210gl,
    env!("CARGO_PKG_DESCRIPTION"),
    plugin_init,
    concat!(env!("CARGO_PKG_VERSION"), "-", env!("COMMIT_ID")),
    "MIT/X11",
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_REPOSITORY"),
    env!("BUILD_REL_DATE")
);

macro_rules! register_element {
    ($ty:ty, $name:literal, $rank:expr) => {
        impl $ty {
            fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
                gst::Element::register(Some(plugin), $name, $rank, Self::static_type())
            }
        }
    };
}

register_element!(V210GlProxy, "v210glproxy", gst::Rank::NONE);
register_element!(V210GlUnproxy, "v210glunproxy", gst::Rank::NONE);
register_element!(V210GlUnpack, "v210glunpack", gst::Rank::NONE);
register_element!(V210GlPack, "v210glpack", gst::Rank::NONE);
register_element!(V210GlUpload, "v210glupload", gst::Rank::PRIMARY);
register_element!(V210GlDownload, "v210gldownload", gst::Rank::PRIMARY);

#[cfg(test)]
mod tests;
