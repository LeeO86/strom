//! Shared GLFilter helpers for unpack/pack shaders.

use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer_gl::prelude::*;
use gstreamer_gl::{GLContext, GLFilter, GLMemory, GLSLProfile, GLSLStage, GLShader};

use crate::properties::ColorSettings;

const GL_FRAGMENT_SHADER: u32 = 0x8B30;

pub fn compile_shader(context: &GLContext, fragment: &str) -> Result<GLShader, glib::Error> {
    let shader = GLShader::new(context);
    let vertex = GLSLStage::new_default_vertex(context);
    shader.compile_attach_stage(&vertex)?;
    let frag = GLSLStage::with_string(
        context,
        GL_FRAGMENT_SHADER,
        gstreamer_gl::GLSLVersion::None,
        GLSLProfile::ES | GLSLProfile::COMPATIBILITY,
        fragment,
    );
    shader.compile_attach_stage(&frag)?;
    shader.link()?;
    Ok(shader)
}

pub fn apply_matrix_uniforms(shader: &GLShader, settings: &ColorSettings) {
    let matrix = settings.matrix();
    let (kr, kb) = matrix.kr_kb();
    shader.set_uniform_1f("u_kr", kr);
    shader.set_uniform_1f("u_kb", kb);
    shader.set_uniform_1f(
        "u_full_range",
        if settings.full_range() { 1.0 } else { 0.0 },
    );
}

pub fn render_with_shader(
    filter: &GLFilter,
    input: &GLMemory,
    output: &GLMemory,
    shader: &GLShader,
) {
    filter.render_to_target_with_shader(input, output, shader);
}

pub fn context_from_filter(filter: &GLFilter) -> Option<GLContext> {
    gstreamer_gl::prelude::GLBaseFilterExt::context(
        filter.upcast_ref::<gstreamer_gl::GLBaseFilter>(),
    )
}
