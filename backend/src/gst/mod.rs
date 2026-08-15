//! GStreamer integration.

mod block_expansion;

/// Register statically-linked plugins and optionally load `gstmxl`.
pub fn register_static_plugins() {
    use tracing::{info, warn};

    gstwebrtchttp::plugin_register_static().expect("Could not register webrtchttp plugins");
    gstrswebrtc::plugin_register_static().expect("Could not register webrtc plugins");
    gstrsinter::plugin_register_static().expect("Could not register inter plugins");
    gstrsrtp::plugin_register_static().expect("Could not register rtp plugins");
    gstrsaudiofx::plugin_register_static().expect("Could not register audiofx plugins");
    gst_plugins_lsp::plugin_register_static().expect("Could not register lsp-dsp-rs plugins");
    gstv210gl::plugin_register_static().expect("Could not register v210gl plugins");
    #[cfg(feature = "efp")]
    gst_plugin_efp::plugin_register_static().expect("Could not register efp mux/demux plugins");

    #[cfg(feature = "mxl")]
    match gstreamer::Plugin::load_by_name("mxl") {
        Ok(_) => info!("Loaded MXL GStreamer plugin (gstmxl)"),
        Err(e) => warn!(
            "MXL plugin not loaded ({e}). Install libgstmxl.so and libmxl.so — see scripts/setup/mxl. MXL blocks stay hidden until the plugin is on GST_PLUGIN_PATH."
        ),
    }
}
pub mod buffer_age_probe;
pub(crate) mod control_bindings;
pub(crate) mod crop;
pub mod discovery;
pub mod pipeline;
pub mod pipeline_monitor;
pub mod shaders;
pub mod thread_priority;
pub mod thumbnail;
pub mod thumbnail_tap;
pub mod transitions;
pub(crate) mod underlay;
pub mod video_frame;
pub mod volume_ramp;
pub mod whep_probe;

pub use discovery::ElementDiscovery;
pub use pipeline::{PipelineError, PipelineManager};
pub use thread_priority::{
    setup_thread_priority_handler, SessionThreadConfig, ThreadPriorityState,
};
pub use thumbnail::ThumbnailError;
pub use thumbnail_tap::{new_tap_store, ThumbnailTap, ThumbnailTapConfig, ThumbnailTapStore};
pub use transitions::{TransitionController, TransitionError, TransitionType};
