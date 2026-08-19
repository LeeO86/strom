//! MXL (Media eXchange Layer) input and output blocks.
//!
//! Video blocks share a `backend` property (`auto` / `gpu` / `cpu`):
//! - GPU: `mxlsrc → queue → v210glupload` and `queue → v210gldownload → mxlsink`
//! - CPU: `videoconvert` instead of the v210 GL elements
//!
//! Audio is a separate pair — MXL audio is F32LE and needs no GPU path.
//!
//! The `gstmxl` plugin is loaded dynamically (it pins gstreamer-rs 0.24;
//! Strom uses 0.25, so it cannot be statically linked). `libmxl.so` must
//! be on the dynamic linker path at runtime.
//!
//! Interlaced v210 is rejected. Insert a deinterlace block between a CPU
//! MXL input and the vision mixer if the flow is interlaced.

use crate::blocks::{BlockBuildContext, BlockBuildError, BlockBuildResult, BlockBuilder};
use crate::gpu;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_base::prelude::*;
use std::collections::HashMap;
use strom_types::mxl::{
    MxlColorimetry, MxlVideoBackend, DEFAULT_MXL_DOMAIN, MXL_AUDIO_INPUT_ID, MXL_AUDIO_OUTPUT_ID,
    MXL_VIDEO_INPUT_ID, MXL_VIDEO_OUTPUT_ID,
};
use strom_types::{block::*, element::ElementPadRef, EnumValue, MediaType, PropertyValue};
use tracing::{info, warn};

fn is_mxl_available() -> bool {
    gst::ElementFactory::find("mxlsrc").is_some() && gst::ElementFactory::find("mxlsink").is_some()
}

fn parse_backend(properties: &HashMap<String, PropertyValue>) -> MxlVideoBackend {
    properties
        .get("backend")
        .and_then(|v| match v {
            PropertyValue::String(s) => Some(MxlVideoBackend::parse(s)),
            _ => None,
        })
        .unwrap_or_default()
}

fn parse_string<'a>(
    properties: &'a HashMap<String, PropertyValue>,
    name: &str,
    default: &'a str,
) -> &'a str {
    properties
        .get(name)
        .and_then(|v| match v {
            PropertyValue::String(s) => Some(s.as_str()),
            _ => None,
        })
        .unwrap_or(default)
}

fn parse_bool(properties: &HashMap<String, PropertyValue>, name: &str, default: bool) -> bool {
    properties
        .get(name)
        .and_then(|v| match v {
            PropertyValue::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(default)
}

fn parse_colorimetry(properties: &HashMap<String, PropertyValue>) -> MxlColorimetry {
    properties
        .get("colorimetry_override")
        .and_then(|v| match v {
            PropertyValue::String(s) => Some(MxlColorimetry::parse(s)),
            _ => None,
        })
        .unwrap_or_default()
}

fn select_video_backend(preference: MxlVideoBackend) -> Result<MxlVideoBackend, BlockBuildError> {
    let has_gl_elements = gst::ElementFactory::find("v210glupload").is_some()
        && gst::ElementFactory::find("v210gldownload").is_some()
        && gst::ElementFactory::find("glupload").is_some();
    let has_hw = gpu::has_hardware_gl();

    match preference {
        MxlVideoBackend::Gpu => {
            if has_gl_elements && has_hw {
                Ok(MxlVideoBackend::Gpu)
            } else {
                Err(BlockBuildError::InvalidConfiguration(
                    "GPU backend requested but hardware GL or v210glupload/v210gldownload is unavailable"
                        .to_string(),
                ))
            }
        }
        MxlVideoBackend::Cpu => Ok(MxlVideoBackend::Cpu),
        MxlVideoBackend::Auto => {
            if has_gl_elements && has_hw {
                Ok(MxlVideoBackend::Gpu)
            } else {
                Ok(MxlVideoBackend::Cpu)
            }
        }
    }
}

fn set_if_present(element: &gst::Element, name: &str, value: &str) {
    if element.find_property(name).is_some() {
        element.set_property(name, value);
    }
}

/// Live MXL output must not preroll against the pipeline clock.
///
/// `mxlsink` inherits BaseSink `sync=true` / `async=true`. Together those leave
/// the pipeline in PAUSED (pending PLAYING): the sink waits for the clock,
/// the clock does not run until PLAYING, and the MXL worker also sleeps on
/// grain PTS. Head index stays 0, and teardown never reaches the writer
/// `destroy()` path, so the flow directory is leaked. Other live sinks in
/// this tree (WHEP, WHIP, AES67, SRT, DeckLink drain) already force
/// `async=false` for the same reason.
///
/// Set these via the BaseSink C API after `ElementFactory::make`:
/// - The UI only serializes modified block properties, so `sync`/`async` are
///   usually absent from `properties` even though the block default is false.
/// - gst-mxl-rs `constructed()` then calls `set_sync(true)`, restoring the
///   deadlock default. `gst-launch … mxlsink sync=false async=false` works
///   because parse applies those after `constructed()`. Strom must do the same.
///
/// `GST_DEBUG=mxlsink:6` never logs "Changing sync/async": those pspecs are
/// owned by GstBaseSink, so GObject dispatches them to the parent class.
/// mxlsink's `set_property` only handles `domain` / `flow-id`. Absence of
/// those debug lines is not proof the values were skipped. Read the log
/// below (`is_sync=` / `is_async=`) instead.
fn configure_mxl_sink_clock(sink: &gst::Element, properties: &HashMap<String, PropertyValue>) {
    let Some(base_sink) = sink.downcast_ref::<gstreamer_base::BaseSink>() else {
        warn!("mxlsink is not a GstBaseSink — cannot set sync/async");
        return;
    };
    let sync = parse_bool(properties, "sync", false);
    let async_state = parse_bool(properties, "async", false);
    base_sink.set_sync(sync);
    base_sink.set_async(async_state);
    info!(
        "mxlsink {}: BaseSink is_sync={} is_async={} (requested sync={} async={})",
        sink.name(),
        base_sink.is_sync(),
        base_sink.is_async(),
        sync,
        async_state
    );
}

fn backend_enum_values() -> Vec<EnumValue> {
    vec![
        EnumValue {
            value: "auto".to_string(),
            label: Some("Auto (GPU first)".to_string()),
        },
        EnumValue {
            value: "gpu".to_string(),
            label: Some("GPU (v210 GL, zero-copy into the mixer)".to_string()),
        },
        EnumValue {
            value: "cpu".to_string(),
            label: Some("CPU (videoconvert)".to_string()),
        },
    ]
}

fn colorimetry_enum_values() -> Vec<EnumValue> {
    vec![
        EnumValue {
            value: "auto".to_string(),
            label: Some("Auto (caps, else BT.709)".to_string()),
        },
        EnumValue {
            value: "bt601".to_string(),
            label: Some("BT.601".to_string()),
        },
        EnumValue {
            value: "bt709".to_string(),
            label: Some("BT.709".to_string()),
        },
        EnumValue {
            value: "bt2020".to_string(),
            label: Some("BT.2020".to_string()),
        },
    ]
}

fn make_queue(id: &str) -> Result<gst::Element, BlockBuildError> {
    gst::ElementFactory::make("queue")
        .name(id)
        .build()
        .map_err(|e| BlockBuildError::ElementCreation(format!("queue: {e}")))
}

/// MXL Video Input.
pub struct MxlVideoInputBuilder;

impl BlockBuilder for MxlVideoInputBuilder {
    fn get_external_pads(
        &self,
        properties: &HashMap<String, PropertyValue>,
    ) -> Option<ExternalPads> {
        let backend =
            select_video_backend(parse_backend(properties)).unwrap_or(MxlVideoBackend::Cpu);
        let internal = match backend {
            MxlVideoBackend::Gpu => "v210glupload",
            _ => "videoconvert",
        };
        Some(ExternalPads {
            inputs: vec![],
            outputs: vec![ExternalPad {
                label: None,
                name: "video_out".to_string(),
                media_type: MediaType::Video,
                internal_element_id: internal.to_string(),
                internal_pad_name: "src".to_string(),
            }],
        })
    }

    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
        _ctx: &BlockBuildContext,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        if !is_mxl_available() {
            return Err(BlockBuildError::ElementCreation(
                "mxlsrc/mxlsink not available — install the MXL SDK and gstmxl plugin".to_string(),
            ));
        }

        let backend = select_video_backend(parse_backend(properties))?;
        let domain = parse_string(properties, "domain", DEFAULT_MXL_DOMAIN);
        let flow_id = parse_string(properties, "video_flow_id", "");
        let colorimetry = parse_colorimetry(properties);
        let full_range = parse_bool(properties, "full_range", false);

        info!(
            "Building MXL Video Input {instance_id} backend={} domain={domain} flow={flow_id}",
            backend.as_str()
        );

        let src_id = format!("{instance_id}:mxlsrc");
        let src = gst::ElementFactory::make("mxlsrc")
            .name(&src_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("mxlsrc: {e}")))?;
        set_if_present(&src, "domain", domain);
        set_if_present(&src, "video-flow-id", flow_id);

        let q_id = format!("{instance_id}:queue");
        let queue = make_queue(&q_id)?;
        let cf_id = format!("{instance_id}:progressive");
        let progressive = gst::Caps::builder("video/x-raw")
            .field("format", "v210")
            .field("interlace-mode", "progressive")
            .build();
        let capsfilter = gst::ElementFactory::make("capsfilter")
            .name(&cf_id)
            .property("caps", progressive)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("capsfilter: {e}")))?;

        let mut elements = vec![
            (src_id.clone(), src),
            (q_id.clone(), queue),
            (cf_id.clone(), capsfilter),
        ];
        let mut links = vec![
            (
                ElementPadRef::pad(&src_id, "src"),
                ElementPadRef::pad(&q_id, "sink"),
            ),
            (
                ElementPadRef::pad(&q_id, "src"),
                ElementPadRef::pad(&cf_id, "sink"),
            ),
        ];

        match backend {
            MxlVideoBackend::Gpu => {
                let up_id = format!("{instance_id}:v210glupload");
                let upload = gst::ElementFactory::make("v210glupload")
                    .name(&up_id)
                    .build()
                    .map_err(|e| BlockBuildError::ElementCreation(format!("v210glupload: {e}")))?;
                if colorimetry != MxlColorimetry::Auto {
                    upload.set_property("colorimetry-override", colorimetry.as_str());
                }
                upload.set_property("full-range", full_range);
                elements.push((up_id.clone(), upload));
                links.push((
                    ElementPadRef::pad(&cf_id, "src"),
                    ElementPadRef::pad(&up_id, "sink"),
                ));
            }
            _ => {
                let conv_id = format!("{instance_id}:videoconvert");
                let conv = gst::ElementFactory::make("videoconvert")
                    .name(&conv_id)
                    .build()
                    .map_err(|e| BlockBuildError::ElementCreation(format!("videoconvert: {e}")))?;
                elements.push((conv_id.clone(), conv));
                links.push((
                    ElementPadRef::pad(&cf_id, "src"),
                    ElementPadRef::pad(&conv_id, "sink"),
                ));
            }
        }

        Ok(BlockBuildResult {
            elements,
            internal_links: links,
            bus_message_handler: None,
            pad_properties: HashMap::new(),
        })
    }
}

/// MXL Video Output.
pub struct MxlVideoOutputBuilder;

impl BlockBuilder for MxlVideoOutputBuilder {
    fn get_external_pads(
        &self,
        _properties: &HashMap<String, PropertyValue>,
    ) -> Option<ExternalPads> {
        Some(ExternalPads {
            inputs: vec![ExternalPad {
                label: None,
                name: "video_in".to_string(),
                media_type: MediaType::Video,
                internal_element_id: "queue".to_string(),
                internal_pad_name: "sink".to_string(),
            }],
            outputs: vec![],
        })
    }

    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
        _ctx: &BlockBuildContext,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        if !is_mxl_available() {
            return Err(BlockBuildError::ElementCreation(
                "mxlsrc/mxlsink not available — install the MXL SDK and gstmxl plugin".to_string(),
            ));
        }

        let backend = select_video_backend(parse_backend(properties))?;
        let domain = parse_string(properties, "domain", DEFAULT_MXL_DOMAIN);
        let flow_id = parse_string(properties, "flow_id", "");
        let label = parse_string(properties, "label", "");
        let description = parse_string(properties, "description", "");
        let group_hint = parse_string(properties, "group_hint", "");
        let colorimetry = parse_colorimetry(properties);
        let full_range = parse_bool(properties, "full_range", false);

        info!(
            "Building MXL Video Output {instance_id} backend={} domain={domain} flow={flow_id}",
            backend.as_str()
        );

        let q_id = format!("{instance_id}:queue");
        let queue = make_queue(&q_id)?;
        let sink_id = format!("{instance_id}:mxlsink");
        let sink = gst::ElementFactory::make("mxlsink")
            .name(&sink_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("mxlsink: {e}")))?;
        set_if_present(&sink, "domain", domain);
        set_if_present(&sink, "flow-id", flow_id);
        if !label.is_empty() {
            set_if_present(&sink, "label", label);
        }
        if !description.is_empty() {
            set_if_present(&sink, "description", description);
        }
        if !group_hint.is_empty() {
            set_if_present(&sink, "group-hint", group_hint);
        }
        configure_mxl_sink_clock(&sink, properties);

        let mut elements = vec![(q_id.clone(), queue), (sink_id.clone(), sink)];
        let mut links = Vec::new();

        match backend {
            MxlVideoBackend::Gpu => {
                let dl_id = format!("{instance_id}:v210gldownload");
                let download = gst::ElementFactory::make("v210gldownload")
                    .name(&dl_id)
                    .build()
                    .map_err(|e| {
                        BlockBuildError::ElementCreation(format!("v210gldownload: {e}"))
                    })?;
                if colorimetry != MxlColorimetry::Auto {
                    download.set_property("colorimetry-override", colorimetry.as_str());
                }
                download.set_property("full-range", full_range);
                elements.insert(1, (dl_id.clone(), download));
                links.push((
                    ElementPadRef::pad(&q_id, "src"),
                    ElementPadRef::pad(&dl_id, "sink"),
                ));
                links.push((
                    ElementPadRef::pad(&dl_id, "src"),
                    ElementPadRef::pad(&sink_id, "sink"),
                ));
            }
            _ => {
                let conv_id = format!("{instance_id}:videoconvert");
                let conv = gst::ElementFactory::make("videoconvert")
                    .name(&conv_id)
                    .build()
                    .map_err(|e| BlockBuildError::ElementCreation(format!("videoconvert: {e}")))?;
                let cf_id = format!("{instance_id}:capsfilter");
                let caps = gst::Caps::builder("video/x-raw")
                    .field("format", "v210")
                    .field("interlace-mode", "progressive")
                    .build();
                let cf = gst::ElementFactory::make("capsfilter")
                    .name(&cf_id)
                    .property("caps", caps)
                    .build()
                    .map_err(|e| BlockBuildError::ElementCreation(format!("capsfilter: {e}")))?;
                elements.insert(1, (conv_id.clone(), conv));
                elements.insert(2, (cf_id.clone(), cf));
                links.push((
                    ElementPadRef::pad(&q_id, "src"),
                    ElementPadRef::pad(&conv_id, "sink"),
                ));
                links.push((
                    ElementPadRef::pad(&conv_id, "src"),
                    ElementPadRef::pad(&cf_id, "sink"),
                ));
                links.push((
                    ElementPadRef::pad(&cf_id, "src"),
                    ElementPadRef::pad(&sink_id, "sink"),
                ));
            }
        }

        Ok(BlockBuildResult {
            elements,
            internal_links: links,
            bus_message_handler: None,
            pad_properties: HashMap::new(),
        })
    }
}

/// MXL Audio Input.
pub struct MxlAudioInputBuilder;

impl BlockBuilder for MxlAudioInputBuilder {
    fn get_external_pads(
        &self,
        _properties: &HashMap<String, PropertyValue>,
    ) -> Option<ExternalPads> {
        Some(ExternalPads {
            inputs: vec![],
            outputs: vec![ExternalPad {
                label: None,
                name: "audio_out".to_string(),
                media_type: MediaType::Audio,
                internal_element_id: "audioconvert".to_string(),
                internal_pad_name: "src".to_string(),
            }],
        })
    }

    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
        _ctx: &BlockBuildContext,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        if !is_mxl_available() {
            return Err(BlockBuildError::ElementCreation(
                "mxlsrc/mxlsink not available — install the MXL SDK and gstmxl plugin".to_string(),
            ));
        }

        let domain = parse_string(properties, "domain", DEFAULT_MXL_DOMAIN);
        let flow_id = parse_string(properties, "audio_flow_id", "");
        info!("Building MXL Audio Input {instance_id} domain={domain} flow={flow_id}");

        let src_id = format!("{instance_id}:mxlsrc");
        let src = gst::ElementFactory::make("mxlsrc")
            .name(&src_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("mxlsrc: {e}")))?;
        set_if_present(&src, "domain", domain);
        set_if_present(&src, "audio-flow-id", flow_id);

        let q_id = format!("{instance_id}:queue");
        let queue = make_queue(&q_id)?;
        let conv_id = format!("{instance_id}:audioconvert");
        let conv = gst::ElementFactory::make("audioconvert")
            .name(&conv_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("audioconvert: {e}")))?;

        Ok(BlockBuildResult {
            elements: vec![
                (src_id.clone(), src),
                (q_id.clone(), queue),
                (conv_id.clone(), conv),
            ],
            internal_links: vec![
                (
                    ElementPadRef::pad(&src_id, "src"),
                    ElementPadRef::pad(&q_id, "sink"),
                ),
                (
                    ElementPadRef::pad(&q_id, "src"),
                    ElementPadRef::pad(&conv_id, "sink"),
                ),
            ],
            bus_message_handler: None,
            pad_properties: HashMap::new(),
        })
    }
}

/// MXL Audio Output.
pub struct MxlAudioOutputBuilder;

impl BlockBuilder for MxlAudioOutputBuilder {
    fn get_external_pads(
        &self,
        _properties: &HashMap<String, PropertyValue>,
    ) -> Option<ExternalPads> {
        Some(ExternalPads {
            inputs: vec![ExternalPad {
                label: None,
                name: "audio_in".to_string(),
                media_type: MediaType::Audio,
                internal_element_id: "queue".to_string(),
                internal_pad_name: "sink".to_string(),
            }],
            outputs: vec![],
        })
    }

    fn build(
        &self,
        instance_id: &str,
        properties: &HashMap<String, PropertyValue>,
        _ctx: &BlockBuildContext,
    ) -> Result<BlockBuildResult, BlockBuildError> {
        if !is_mxl_available() {
            return Err(BlockBuildError::ElementCreation(
                "mxlsrc/mxlsink not available — install the MXL SDK and gstmxl plugin".to_string(),
            ));
        }

        let domain = parse_string(properties, "domain", DEFAULT_MXL_DOMAIN);
        let flow_id = parse_string(properties, "flow_id", "");
        let label = parse_string(properties, "label", "");
        let description = parse_string(properties, "description", "");
        let group_hint = parse_string(properties, "group_hint", "");
        info!("Building MXL Audio Output {instance_id} domain={domain} flow={flow_id}");

        let q_id = format!("{instance_id}:queue");
        let queue = make_queue(&q_id)?;
        let conv_id = format!("{instance_id}:audioconvert");
        let conv = gst::ElementFactory::make("audioconvert")
            .name(&conv_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("audioconvert: {e}")))?;
        let sink_id = format!("{instance_id}:mxlsink");
        let sink = gst::ElementFactory::make("mxlsink")
            .name(&sink_id)
            .build()
            .map_err(|e| BlockBuildError::ElementCreation(format!("mxlsink: {e}")))?;
        set_if_present(&sink, "domain", domain);
        set_if_present(&sink, "flow-id", flow_id);
        if !label.is_empty() {
            set_if_present(&sink, "label", label);
        }
        if !description.is_empty() {
            set_if_present(&sink, "description", description);
        }
        if !group_hint.is_empty() {
            set_if_present(&sink, "group-hint", group_hint);
        }
        configure_mxl_sink_clock(&sink, properties);

        Ok(BlockBuildResult {
            elements: vec![
                (q_id.clone(), queue),
                (conv_id.clone(), conv),
                (sink_id.clone(), sink),
            ],
            internal_links: vec![
                (
                    ElementPadRef::pad(&q_id, "src"),
                    ElementPadRef::pad(&conv_id, "sink"),
                ),
                (
                    ElementPadRef::pad(&conv_id, "src"),
                    ElementPadRef::pad(&sink_id, "sink"),
                ),
            ],
            bus_message_handler: None,
            pad_properties: HashMap::new(),
        })
    }
}

fn video_precision_note() -> &'static str {
    "The vision mixer GL path composites in 8-bit RGBA, so a 10-bit v210 round-trip is not bit-exact. \
     Interlaced MXL video is rejected — insert a deinterlace block on the CPU path if needed. \
     Ancillary ST 2038 data is out of scope."
}

pub fn get_blocks() -> Vec<BlockDefinition> {
    if !is_mxl_available() {
        warn!("MXL GStreamer plugin not available — hiding MXL blocks from the palette");
        return vec![];
    }
    info!("MXL GStreamer plugin detected — enabling MXL blocks");
    vec![
        video_input_definition(),
        video_output_definition(),
        audio_input_definition(),
        audio_output_definition(),
    ]
}

fn video_input_definition() -> BlockDefinition {
    BlockDefinition {
        id: MXL_VIDEO_INPUT_ID.to_string(),
        name: "MXL Video Input".to_string(),
        description: format!(
            "Reads an MXL video/v210 flow. GPU backend uploads via v210glupload (zero-copy proxy texture). {}",
            video_precision_note()
        ),
        category: "Inputs".to_string(),
        exposed_properties: vec![
            string_prop("domain", "Domain", "Filesystem path of the MXL domain directory", DEFAULT_MXL_DOMAIN, "mxlsrc", "domain"),
            string_prop("video_flow_id", "Video Flow ID", "UUID of the MXL video flow to read", "", "mxlsrc", "video-flow-id"),
            enum_prop("backend", "Backend", "GPU (v210 GL), CPU (videoconvert), or auto", "auto", backend_enum_values()),
            enum_prop("colorimetry_override", "Colorimetry", "YCbCr matrix used by the GPU unpack shader", "auto", colorimetry_enum_values()),
            bool_prop("full_range", "Full range", "Treat YCbCr as full range (default is limited/narrow)", false),
        ],
        external_pads: ExternalPads {
            inputs: vec![],
            outputs: vec![ExternalPad {
                label: None,
                name: "video_out".to_string(),
                media_type: MediaType::Video,
                internal_element_id: "v210glupload".to_string(),
                internal_pad_name: "src".to_string(),
            }],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("🎞".to_string()),
            width: Some(2.0),
            height: Some(1.2),
            ..Default::default()
        }),
    }
}

fn video_output_definition() -> BlockDefinition {
    BlockDefinition {
        id: MXL_VIDEO_OUTPUT_ID.to_string(),
        name: "MXL Video Output".to_string(),
        description: format!(
            "Writes a video/v210 MXL flow. GPU backend packs via v210gldownload. {}",
            video_precision_note()
        ),
        category: "Outputs".to_string(),
        exposed_properties: vec![
            string_prop(
                "domain",
                "Domain",
                "Filesystem path of the MXL domain directory",
                DEFAULT_MXL_DOMAIN,
                "mxlsink",
                "domain",
            ),
            string_prop(
                "flow_id",
                "Flow ID",
                "UUID of the MXL video flow to create",
                "",
                "mxlsink",
                "flow-id",
            ),
            string_prop(
                "label",
                "Label",
                "Optional flow_def label",
                "",
                "mxlsink",
                "label",
            ),
            string_prop(
                "description",
                "Description",
                "Optional flow_def description",
                "",
                "mxlsink",
                "description",
            ),
            string_prop(
                "group_hint",
                "Group hint",
                "Optional NMOS group hint (e.g. Camera:Video)",
                "",
                "mxlsink",
                "group-hint",
            ),
            enum_prop(
                "backend",
                "Backend",
                "GPU (v210 GL), CPU (videoconvert), or auto",
                "auto",
                backend_enum_values(),
            ),
            enum_prop(
                "colorimetry_override",
                "Colorimetry",
                "YCbCr matrix used by the GPU pack shader",
                "auto",
                colorimetry_enum_values(),
            ),
            bool_prop(
                "full_range",
                "Full range",
                "Encode YCbCr as full range (default is limited/narrow)",
                false,
            ),
            bool_prop(
                "sync",
                "Sync to clock",
                "BaseSink sync. Leave off for live MXL output — on (with async) deadlocks preroll, writes no grains, and leaks the flow directory",
                false,
            ),
            bool_prop(
                "async",
                "Async state change",
                "BaseSink async. Leave off so the pipeline can reach PLAYING without waiting for the first synced buffer",
                false,
            ),
        ],
        external_pads: ExternalPads {
            inputs: vec![ExternalPad {
                label: None,
                name: "video_in".to_string(),
                media_type: MediaType::Video,
                internal_element_id: "queue".to_string(),
                internal_pad_name: "sink".to_string(),
            }],
            outputs: vec![],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("📤".to_string()),
            width: Some(2.0),
            height: Some(1.2),
            ..Default::default()
        }),
    }
}

fn audio_input_definition() -> BlockDefinition {
    BlockDefinition {
        id: MXL_AUDIO_INPUT_ID.to_string(),
        name: "MXL Audio Input".to_string(),
        description: "Reads an MXL audio/float32 flow (F32LE). Channel count follows the flow; do not assume stereo.".to_string(),
        category: "Inputs".to_string(),
        exposed_properties: vec![
            string_prop("domain", "Domain", "Filesystem path of the MXL domain directory", DEFAULT_MXL_DOMAIN, "mxlsrc", "domain"),
            string_prop("audio_flow_id", "Audio Flow ID", "UUID of the MXL audio flow to read", "", "mxlsrc", "audio-flow-id"),
        ],
        external_pads: ExternalPads {
            inputs: vec![],
            outputs: vec![ExternalPad {
                label: None,
                name: "audio_out".to_string(),
                media_type: MediaType::Audio,
                internal_element_id: "audioconvert".to_string(),
                internal_pad_name: "src".to_string(),
            }],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("🔊".to_string()),
            width: Some(2.0),
            height: Some(1.2),
            ..Default::default()
        }),
    }
}

fn audio_output_definition() -> BlockDefinition {
    BlockDefinition {
        id: MXL_AUDIO_OUTPUT_ID.to_string(),
        name: "MXL Audio Output".to_string(),
        description:
            "Writes an MXL audio/float32 flow (F32LE). Channel count follows the incoming caps."
                .to_string(),
        category: "Outputs".to_string(),
        exposed_properties: vec![
            string_prop(
                "domain",
                "Domain",
                "Filesystem path of the MXL domain directory",
                DEFAULT_MXL_DOMAIN,
                "mxlsink",
                "domain",
            ),
            string_prop(
                "flow_id",
                "Flow ID",
                "UUID of the MXL audio flow to create",
                "",
                "mxlsink",
                "flow-id",
            ),
            string_prop(
                "label",
                "Label",
                "Optional flow_def label",
                "",
                "mxlsink",
                "label",
            ),
            string_prop(
                "description",
                "Description",
                "Optional flow_def description",
                "",
                "mxlsink",
                "description",
            ),
            string_prop(
                "group_hint",
                "Group hint",
                "Optional NMOS group hint (e.g. Mixer:Audio)",
                "",
                "mxlsink",
                "group-hint",
            ),
            bool_prop(
                "sync",
                "Sync to clock",
                "BaseSink sync. Leave off for live MXL output — on (with async) deadlocks preroll, writes no grains, and leaks the flow directory",
                false,
            ),
            bool_prop(
                "async",
                "Async state change",
                "BaseSink async. Leave off so the pipeline can reach PLAYING without waiting for the first synced buffer",
                false,
            ),
        ],
        external_pads: ExternalPads {
            inputs: vec![ExternalPad {
                label: None,
                name: "audio_in".to_string(),
                media_type: MediaType::Audio,
                internal_element_id: "queue".to_string(),
                internal_pad_name: "sink".to_string(),
            }],
            outputs: vec![],
        },
        built_in: true,
        ui_metadata: Some(BlockUIMetadata {
            icon: Some("📤".to_string()),
            width: Some(2.0),
            height: Some(1.2),
            ..Default::default()
        }),
    }
}

fn string_prop(
    name: &str,
    label: &str,
    description: &str,
    default: &str,
    element_id: &str,
    property_name: &str,
) -> ExposedProperty {
    ExposedProperty {
        name: name.to_string(),
        label: label.to_string(),
        description: description.to_string(),
        property_type: PropertyType::String,
        default_value: Some(PropertyValue::String(default.to_string())),
        mapping: PropertyMapping {
            element_id: element_id.to_string(),
            property_name: property_name.to_string(),
            transform: None,
        },
        live: false,
        persist: None,
    }
}

fn enum_prop(
    name: &str,
    label: &str,
    description: &str,
    default: &str,
    values: Vec<EnumValue>,
) -> ExposedProperty {
    ExposedProperty {
        name: name.to_string(),
        label: label.to_string(),
        description: description.to_string(),
        property_type: PropertyType::Enum { values },
        default_value: Some(PropertyValue::String(default.to_string())),
        mapping: PropertyMapping {
            element_id: "_block".to_string(),
            property_name: name.to_string(),
            transform: None,
        },
        live: false,
        persist: None,
    }
}

fn bool_prop(name: &str, label: &str, description: &str, default: bool) -> ExposedProperty {
    ExposedProperty {
        name: name.to_string(),
        label: label.to_string(),
        description: description.to_string(),
        property_type: PropertyType::Bool,
        default_value: Some(PropertyValue::Bool(default)),
        mapping: PropertyMapping {
            element_id: "_block".to_string(),
            property_name: name.to_string(),
            transform: None,
        },
        live: false,
        persist: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init() {
        let _ = gst::init();
        let _ = gstv210gl::plugin_register_static();
        #[cfg(feature = "mxl")]
        let _ = gst::Plugin::load_by_name("mxl");
    }

    #[test]
    fn backend_parse() {
        assert_eq!(MxlVideoBackend::parse("gpu"), MxlVideoBackend::Gpu);
        assert_eq!(MxlVideoBackend::parse("cpu"), MxlVideoBackend::Cpu);
        assert_eq!(MxlVideoBackend::parse("auto"), MxlVideoBackend::Auto);
        assert_eq!(MxlVideoBackend::parse("nope"), MxlVideoBackend::Auto);
    }

    #[test]
    fn definitions_have_valid_defaults() {
        init();
        // Palette may be empty without gstmxl; definitions are still constructed here.
        let defs = [
            video_input_definition(),
            video_output_definition(),
            audio_input_definition(),
            audio_output_definition(),
        ];
        for def in defs {
            assert!(def.id.starts_with("builtin.mxl_"));
            for prop in &def.exposed_properties {
                assert!(!prop.name.is_empty());
                assert!(prop.default_value.is_some());
            }
        }
        for def in [video_output_definition(), audio_output_definition()] {
            let sync = def
                .exposed_properties
                .iter()
                .find(|p| p.name == "sync")
                .expect("mxlsink sync");
            let async_prop = def
                .exposed_properties
                .iter()
                .find(|p| p.name == "async")
                .expect("mxlsink async");
            assert!(
                matches!(sync.default_value, Some(PropertyValue::Bool(false))),
                "sync default must be false"
            );
            assert!(
                matches!(async_prop.default_value, Some(PropertyValue::Bool(false))),
                "async default must be false"
            );
        }
    }

    #[test]
    fn v210_elements_register() {
        init();
        assert!(gst::ElementFactory::find("v210glupload").is_some());
        assert!(gst::ElementFactory::find("v210gldownload").is_some());
    }

    fn find_mxl_sink(result: &BlockBuildResult) -> gst::Element {
        result
            .elements
            .iter()
            .find(|(id, _)| id.ends_with(":mxlsink"))
            .map(|(_, e)| e.clone())
            .expect("block result must contain mxlsink")
    }

    fn assert_live_mxl_clock(sink: &gst::Element) {
        let base = sink
            .downcast_ref::<gstreamer_base::BaseSink>()
            .expect("mxlsink is a GstBaseSink");
        assert!(
            !base.is_sync(),
            "live mxlsink must have BaseSink sync=false (got true; GObject set_property is swallowed)"
        );
        assert!(
            !base.is_async(),
            "live mxlsink must have BaseSink async=false (got true; GObject set_property is swallowed)"
        );
    }

    #[test]
    fn configure_mxl_sink_clock_overrides_basesink_defaults_when_properties_empty() {
        init();
        // appsink keeps BaseSink's sync=true/async=true (fakesink overrides sync to false).
        let sink = gst::ElementFactory::make("appsink")
            .build()
            .expect("appsink");
        {
            let base = sink
                .downcast_ref::<gstreamer_base::BaseSink>()
                .expect("appsink is a GstBaseSink");
            assert!(base.is_sync(), "appsink default sync is true");
            assert!(base.is_async(), "appsink default async is true");
        }
        configure_mxl_sink_clock(&sink, &HashMap::new());
        assert_live_mxl_clock(&sink);
    }

    #[test]
    fn video_output_forces_sync_async_off_when_ui_omits_unmodified_defaults() {
        init();
        if !is_mxl_available() {
            eprintln!("gstmxl plugin not available — skipping");
            return;
        }
        let ctx = BlockBuildContext::new(vec![], "all".to_string());
        let result = MxlVideoOutputBuilder
            .build("test_mxl_vo", &HashMap::new(), &ctx)
            .expect("empty properties must still build (UI omits unmodified defaults)");
        assert_live_mxl_clock(&find_mxl_sink(&result));
    }

    #[test]
    fn audio_output_forces_sync_async_off_when_ui_omits_unmodified_defaults() {
        init();
        if !is_mxl_available() {
            eprintln!("gstmxl plugin not available — skipping");
            return;
        }
        let ctx = BlockBuildContext::new(vec![], "all".to_string());
        let result = MxlAudioOutputBuilder
            .build("test_mxl_ao", &HashMap::new(), &ctx)
            .expect("empty properties must still build (UI omits unmodified defaults)");
        assert_live_mxl_clock(&find_mxl_sink(&result));
    }
}
