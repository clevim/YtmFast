//! Video: mpv renders the music video straight into a GL texture egui draws.
//!
//! Uses the C render API directly: the libmpv2 wrapper can't turn off
//! BLOCK_FOR_TARGET_TIME, which made every render stall the UI thread for a frame.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::Arc;

use eframe::egui;
use eframe::glow::{self, HasContext};
use libmpv2::Mpv;
use libmpv2_sys as sys;

pub type Gpa = Arc<dyn Fn(&CStr) -> *const c_void + Send + Sync>;

unsafe extern "C" fn get_proc_address(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: ctx is the boxed Gpa owned by `Vis`, alive as long as the render context.
    unsafe { (*(ctx as *const Gpa))(CStr::from_ptr(name)) as *mut c_void }
}

unsafe extern "C" fn on_update(ctx: *mut c_void) {
    // New visualizer frame -> repaint. No video stream = no frames = no repaints.
    unsafe { (*(ctx as *const egui::Context)).request_repaint() }
}

struct Target {
    fbo: glow::Framebuffer,
    tex: glow::Texture,
    id: egui::TextureId,
    size: [i32; 2],
}

pub struct Vis {
    rc: *mut sys::mpv_render_context,
    target: Option<Target>,
    // Pointed to by the render context's callbacks.
    _gpa: Box<Gpa>,
    _ctx: Box<egui::Context>,
}

fn param(type_: sys::mpv_render_param_type, data: *mut c_void) -> sys::mpv_render_param {
    sys::mpv_render_param { type_, data }
}

impl Vis {
    pub fn new(mpv: &Mpv, gpa: Gpa, ctx: egui::Context) -> Option<Self> {
        let gpa = Box::new(gpa);
        let ctx = Box::new(ctx);
        let mut init = sys::mpv_opengl_init_params {
            get_proc_address: Some(get_proc_address),
            get_proc_address_ctx: &*gpa as *const Gpa as *mut c_void,
        };
        let mut params = [
            param(sys::mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE, sys::MPV_RENDER_API_TYPE_OPENGL.as_ptr() as *mut c_void),
            param(sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, &mut init as *mut _ as *mut c_void),
            param(0, std::ptr::null_mut()),
        ];
        let mut rc = std::ptr::null_mut();
        // SAFETY: params are valid for the call; eframe's GL context is current during creation.
        unsafe {
            if sys::mpv_render_context_create(&mut rc, mpv.ctx.as_ptr(), params.as_mut_ptr()) < 0 {
                return None;
            }
            sys::mpv_render_context_set_update_callback(rc, Some(on_update), &*ctx as *const egui::Context as *mut c_void);
        }
        Some(Self { rc, target: None, _gpa: gpa, _ctx: ctx })
    }

    /// Renders the latest frame at `size` pixels. Returns the texture to draw and whether
    /// a new frame landed in it.
    pub fn frame(&mut self, gl: &glow::Context, frame: &mut eframe::Frame, size: [i32; 2]) -> (egui::TextureId, bool) {
        let size = [size[0].max(1), size[1].max(1)];
        // SAFETY: plain GL / mpv render calls on eframe's current context, from the UI thread.
        unsafe {
            if self.target.is_none() {
                let tex = gl.create_texture().expect("gl texture");
                let fbo = gl.create_framebuffer().expect("gl framebuffer");
                let id = frame.register_native_glow_texture(tex);
                self.target = Some(Target { fbo, tex, id, size: [0, 0] });
            }
            let t = self.target.as_mut().unwrap();
            if t.size != size {
                // Same texture object, new storage: the egui TextureId stays valid.
                gl.bind_texture(glow::TEXTURE_2D, Some(t.tex));
                gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, size[0], size[1], 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(None));
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(t.fbo));
                gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(t.tex), 0);
                gl.clear_color(0.0, 0.0, 0.0, 0.0);
                gl.clear(glow::COLOR_BUFFER_BIT);
                gl.bind_framebuffer(glow::FRAMEBUFFER, None);
                t.size = size;
            }
            let fresh = sys::mpv_render_context_update(self.rc) & sys::mpv_render_update_flag_MPV_RENDER_UPDATE_FRAME as u64 != 0;
            if fresh {
                let mut fbo = sys::mpv_opengl_fbo { fbo: t.fbo.0.get() as c_int, w: size[0], h: size[1], internal_format: 0 };
                let (mut flip, mut block): (c_int, c_int) = (0, 0);
                let mut params = [
                    param(sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_FBO, &mut fbo as *mut _ as *mut c_void),
                    param(sys::mpv_render_param_type_MPV_RENDER_PARAM_FLIP_Y, &mut flip as *mut _ as *mut c_void),
                    param(sys::mpv_render_param_type_MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME, &mut block as *mut _ as *mut c_void),
                    param(0, std::ptr::null_mut()),
                ];
                sys::mpv_render_context_render(self.rc, params.as_mut_ptr());
                gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            }
            (t.id, fresh)
        }
    }
}

impl Drop for Vis {
    fn drop(&mut self) {
        // SAFETY: created in `new`, freed once.
        unsafe { sys::mpv_render_context_free(self.rc) }
    }
}
