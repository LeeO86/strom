//! Shared GObject properties for the v210 GL elements.

use gstreamer::glib;
use gstreamer::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::color::Matrix;

pub const COLORIMETRY_OVERRIDE: &[&str] = &["auto", "bt601", "bt709", "bt2020"];

#[derive(Debug)]
pub struct ColorSettings {
    pub override_idx: AtomicI32,
    pub full_range: AtomicBool,
    pub video_width: AtomicI32,
    pub video_height: AtomicI32,
}

impl Default for ColorSettings {
    fn default() -> Self {
        Self {
            override_idx: AtomicI32::new(0),
            full_range: AtomicBool::new(false),
            video_width: AtomicI32::new(0),
            video_height: AtomicI32::new(0),
        }
    }
}

impl ColorSettings {
    pub fn matrix(&self) -> Matrix {
        match self.override_idx.load(Ordering::Relaxed) {
            1 => Matrix::Bt601,
            3 => Matrix::Bt2020,
            _ => Matrix::Bt709,
        }
    }

    pub fn override_is_auto(&self) -> bool {
        self.override_idx.load(Ordering::Relaxed) == 0
    }

    pub fn set_override_from_string(&self, value: &str) {
        let idx = match value {
            "bt601" => 1,
            "bt709" => 2,
            "bt2020" => 3,
            _ => 0,
        };
        self.override_idx.store(idx, Ordering::Relaxed);
    }

    pub fn override_string(&self) -> &'static str {
        COLORIMETRY_OVERRIDE[self.override_idx.load(Ordering::Relaxed) as usize]
    }

    pub fn full_range(&self) -> bool {
        self.full_range.load(Ordering::Relaxed)
    }

    pub fn video_size(&self) -> (u32, u32) {
        (
            self.video_width.load(Ordering::Relaxed).max(0) as u32,
            self.video_height.load(Ordering::Relaxed).max(0) as u32,
        )
    }
}

pub fn color_properties() -> Vec<glib::ParamSpec> {
    vec![
        glib::ParamSpecString::builder("colorimetry-override")
            .nick("Colorimetry override")
            .blurb("YCbCr matrix: auto (BT.709 default, or caps colorimetry), bt601, bt709, bt2020")
            .default_value(Some("auto"))
            .build(),
        glib::ParamSpecBoolean::builder("full-range")
            .nick("Full range")
            .blurb("Interpret YCbCr as full range (false = limited/narrow, the broadcast default)")
            .default_value(false)
            .build(),
        glib::ParamSpecInt::builder("video-width")
            .nick("Video width")
            .blurb("Original v210 width in pixels (set by the parent bin from sink caps)")
            .minimum(0)
            .maximum(i32::MAX)
            .default_value(0)
            .build(),
        glib::ParamSpecInt::builder("video-height")
            .nick("Video height")
            .blurb("Original v210 height in pixels (set by the parent bin from sink caps)")
            .minimum(0)
            .maximum(i32::MAX)
            .default_value(0)
            .build(),
    ]
}

pub fn set_color_property(settings: &ColorSettings, value: &glib::Value, pspec: &glib::ParamSpec) {
    match pspec.name() {
        "colorimetry-override" => {
            if let Ok(s) = value.get::<Option<String>>() {
                settings.set_override_from_string(s.as_deref().unwrap_or("auto"));
            }
        }
        "full-range" => {
            if let Ok(v) = value.get::<bool>() {
                settings.full_range.store(v, Ordering::Relaxed);
            }
        }
        "video-width" => {
            if let Ok(v) = value.get::<i32>() {
                settings.video_width.store(v, Ordering::Relaxed);
            }
        }
        "video-height" => {
            if let Ok(v) = value.get::<i32>() {
                settings.video_height.store(v, Ordering::Relaxed);
            }
        }
        _ => {}
    }
}

pub fn get_color_property(settings: &ColorSettings, pspec: &glib::ParamSpec) -> glib::Value {
    match pspec.name() {
        "colorimetry-override" => settings.override_string().to_value(),
        "full-range" => settings.full_range().to_value(),
        "video-width" => settings.video_width.load(Ordering::Relaxed).to_value(),
        "video-height" => settings.video_height.load(Ordering::Relaxed).to_value(),
        _ => "".to_value(),
    }
}
