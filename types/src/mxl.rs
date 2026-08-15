//! Shared constants and enums for MXL input/output blocks.

/// Default MXL domain directory (tmpfs namespace used by the MXL SDK).
pub const DEFAULT_MXL_DOMAIN: &str = "/dev/shm/mxl";

/// Block definition IDs.
pub const MXL_VIDEO_INPUT_ID: &str = "builtin.mxl_video_input";
pub const MXL_VIDEO_OUTPUT_ID: &str = "builtin.mxl_video_output";
pub const MXL_AUDIO_INPUT_ID: &str = "builtin.mxl_audio_input";
pub const MXL_AUDIO_OUTPUT_ID: &str = "builtin.mxl_audio_output";

/// Backend selection for MXL video blocks (`auto` / `gpu` / `cpu`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MxlVideoBackend {
    #[default]
    Auto,
    Gpu,
    Cpu,
}

impl MxlVideoBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Gpu => "gpu",
            Self::Cpu => "cpu",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "gpu" => Self::Gpu,
            "cpu" => Self::Cpu,
            _ => Self::Auto,
        }
    }
}

/// Colorimetry override for the v210 GL elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MxlColorimetry {
    #[default]
    Auto,
    Bt601,
    Bt709,
    Bt2020,
}

impl MxlColorimetry {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Bt601 => "bt601",
            Self::Bt709 => "bt709",
            Self::Bt2020 => "bt2020",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "bt601" => Self::Bt601,
            "bt2020" => Self::Bt2020,
            "bt709" => Self::Bt709,
            _ => Self::Auto,
        }
    }
}
