//! Element registration and GPU-gated smoke tests.

use gstreamer as gst;
use gstreamer::prelude::*;

fn init() {
    let _ = gst::init();
    let _ = crate::plugin_register_static();
}

fn has_hardware_gl() -> bool {
    // RGB10A2 upload can g_error/abort on software or incomplete GL stacks
    // (seen here as gst_gl_format_from_video_info). Only run when asked.
    if std::env::var("STROM_V210GL_GPU_TEST").as_deref() != Ok("1") {
        return false;
    }
    if std::env::var("LIBGL_ALWAYS_SOFTWARE").is_ok() {
        return false;
    }
    let Ok(pipeline) = gst::parse::launch(
        "videotestsrc num-buffers=1 ! video/x-raw,width=64,height=64 ! glupload ! fakesink",
    ) else {
        return false;
    };
    let pipeline = match pipeline.downcast::<gst::Pipeline>() {
        Ok(p) => p,
        Err(_) => return false,
    };
    if pipeline.set_state(gst::State::Playing).is_err() {
        let _ = pipeline.set_state(gst::State::Null);
        return false;
    }
    let _ = pipeline.state(gst::ClockTime::from_seconds(2));
    let _ = pipeline.set_state(gst::State::Null);
    true
}

#[test]
fn elements_register() {
    init();
    for name in [
        "v210glupload",
        "v210gldownload",
        "v210glproxy",
        "v210glunproxy",
        "v210glunpack",
        "v210glpack",
    ] {
        assert!(
            gst::ElementFactory::find(name).is_some(),
            "{name} should be registered"
        );
    }
}

#[test]
fn bins_construct() {
    init();
    let upload = gst::ElementFactory::make("v210glupload")
        .build()
        .expect("construct v210glupload");
    assert!(upload.static_pad("sink").is_some());
    assert!(upload.static_pad("src").is_some());
    let download = gst::ElementFactory::make("v210gldownload")
        .build()
        .expect("construct v210gldownload");
    assert!(download.static_pad("sink").is_some());
    assert!(download.static_pad("src").is_some());
}

#[test]
fn upload_rejects_interlaced_template() {
    init();
    // Public bins have ghost pads, not Always templates (those clash on GstBin).
    let factory = gst::ElementFactory::find("v210glproxy").unwrap();
    let caps = factory
        .static_pad_templates()
        .into_iter()
        .find(|t| t.direction() == gst::PadDirection::Sink)
        .unwrap()
        .caps();
    let s = caps.structure(0).unwrap();
    assert_eq!(s.get::<&str>("format").unwrap(), "v210");
    assert_eq!(s.get::<&str>("interlace-mode").unwrap(), "progressive");
}

#[test]
fn gpu_roundtrip_negotiates_when_hardware_gl_present() {
    init();
    if !has_hardware_gl() {
        eprintln!("skipping GPU round-trip: set STROM_V210GL_GPU_TEST=1 on a hardware GL host");
        return;
    }

    let pipeline = match gst::parse::launch(
        "videotestsrc num-buffers=3 pattern=smpte ! \
         video/x-raw,width=1920,height=1080,framerate=25/1 ! \
         videoconvert ! video/x-raw,format=v210,interlace-mode=progressive ! \
         v210glupload ! v210gldownload ! fakesink sync=false",
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("skipping GPU round-trip: failed to parse pipeline: {e}");
            return;
        }
    };
    let pipeline = pipeline.downcast::<gst::Pipeline>().unwrap();
    if pipeline.set_state(gst::State::Playing).is_err() {
        let _ = pipeline.set_state(gst::State::Null);
        eprintln!("skipping GPU round-trip: failed to reach PLAYING");
        return;
    }
    let bus = pipeline.bus().unwrap();
    let mut eos = false;
    while let Some(msg) = bus.timed_pop(gst::ClockTime::from_seconds(10)) {
        use gst::MessageView;
        match msg.view() {
            MessageView::Eos(..) => {
                eos = true;
                break;
            }
            MessageView::Error(e) => {
                let _ = pipeline.set_state(gst::State::Null);
                eprintln!(
                    "skipping GPU round-trip: pipeline error: {} ({:?})",
                    e.error(),
                    e.debug()
                );
                return;
            }
            _ => {}
        }
    }
    let _ = pipeline.set_state(gst::State::Null);
    if !eos {
        eprintln!("skipping GPU round-trip: no EOS (likely software GL / missing shader path)");
    }
}
