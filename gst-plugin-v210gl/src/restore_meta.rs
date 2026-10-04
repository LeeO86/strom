//! Puts a pooled buffer's `GstVideoMeta` back when the buffer returns to its pool.
//!
//! v210glproxy and v210glunproxy relabel a buffer between v210 and RGB10A2
//! without copying it. A pool marks its metas LOCKED, so they rewrite the
//! VideoMeta in place. The pool then hands the same buffer out again with the
//! rewritten meta: v210glpack mapped its output with RGB10A2 caps and a V210
//! meta, the map failed and the MXL GPU flow stopped with a flow error. When a
//! buffer comes back, its pool removes every meta it did not add; the meta
//! defined here restores the VideoMeta at that moment.

use std::ptr::NonNull;
use std::sync::LazyLock;

use gstreamer as gst;
use gstreamer::glib;
use gstreamer_video as gst_video;

#[derive(Clone, Copy)]
struct Saved {
    video_meta: *mut gst_video::ffi::GstVideoMeta,
    format: gst_video::ffi::GstVideoFormat,
    width: u32,
    n_planes: u32,
}

#[repr(C)]
struct RestoreMeta {
    parent: gst::ffi::GstMeta,
    saved: Saved,
}

unsafe extern "C" fn restore_meta_init(
    meta: *mut gst::ffi::GstMeta,
    params: glib::ffi::gpointer,
    _buffer: *mut gst::ffi::GstBuffer,
) -> glib::ffi::gboolean {
    std::ptr::addr_of_mut!((*(meta as *mut RestoreMeta)).saved).write(*(params as *const Saved));
    glib::ffi::GTRUE
}

unsafe extern "C" fn restore_meta_free(
    meta: *mut gst::ffi::GstMeta,
    buffer: *mut gst::ffi::GstBuffer,
) {
    // A buffer that is being freed (refcount 0) drops its metas one by one and
    // the VideoMeta may be gone already; nothing needs restoring then. A pool
    // removes this meta from a live buffer.
    if (*buffer).mini_object.refcount == 0 {
        return;
    }
    let saved = (*(meta as *const RestoreMeta)).saved;
    (*saved.video_meta).format = saved.format;
    (*saved.video_meta).width = saved.width;
    (*saved.video_meta).n_planes = saved.n_planes;
}

static API: LazyLock<glib::ffi::GType> = LazyLock::new(|| unsafe {
    let mut tags = [std::ptr::null::<std::os::raw::c_char>()];
    gst::ffi::gst_meta_api_type_register(c"V210GlRestoreVideoMetaAPI".as_ptr(), tags.as_mut_ptr())
});

struct MetaInfo(NonNull<gst::ffi::GstMetaInfo>);
// SAFETY: a registered GstMetaInfo is immutable and lives for the process.
unsafe impl Send for MetaInfo {}
unsafe impl Sync for MetaInfo {}

static INFO: LazyLock<MetaInfo> = LazyLock::new(|| unsafe {
    let info = gst::ffi::gst_meta_register(
        *API,
        c"V210GlRestoreVideoMeta".as_ptr(),
        std::mem::size_of::<RestoreMeta>(),
        Some(restore_meta_init),
        Some(restore_meta_free),
        // Not copied: a copy owns its own, unlocked VideoMeta.
        None,
    );
    MetaInfo(NonNull::new(info as *mut _).expect("register V210GlRestoreVideoMeta"))
});

/// Remember the current values of `video_meta` so they come back when `buf`
/// returns to its pool. Only the first call per buffer records anything.
///
/// # Safety
/// `video_meta` must be a LOCKED VideoMeta of `buf`.
pub unsafe fn save(buf: &mut gst::BufferRef, video_meta: *mut gst_video::ffi::GstVideoMeta) {
    if !gst::ffi::gst_buffer_get_meta(buf.as_mut_ptr(), *API).is_null() {
        return;
    }
    let mut saved = Saved {
        video_meta,
        format: (*video_meta).format,
        width: (*video_meta).width,
        n_planes: (*video_meta).n_planes,
    };
    gst::ffi::gst_buffer_add_meta(
        buf.as_mut_ptr(),
        INFO.0.as_ptr(),
        &mut saved as *mut Saved as glib::ffi::gpointer,
    );
}
