use super::types::{color, mxcfb_rect};
use cgmath::Point2;
use image::RgbImage;
use std::os::unix::io::RawFd;

// Embed fonts for text rendering
const FONT_REGULAR: &[u8] = include_bytes!("../../../../res/NotoSans-Regular.ttf");

// QTFB protocol constants
const SOCKET_PATH: &[u8] = b"/tmp/qtfb.sock\0";
const DEFAULT_FB_KEY: u32 = 245209899;

const MESSAGE_INITIALIZE: u8 = 0;
const MESSAGE_UPDATE: u8 = 1;
const MESSAGE_SET_REFRESH_MODE: u8 = 5;
const MESSAGE_REQUEST_FULL_REFRESH: u8 = 6;

const UPDATE_ALL: i32 = 0;
const UPDATE_PARTIAL: i32 = 1;

const FBFMT_RMPP_RGB888: u8 = 1;

const REFRESH_MODE_FAST: i32 = 1;
const REFRESH_MODE_CONTENT: i32 = 3;
#[allow(dead_code)]
const REFRESH_MODE_UI: i32 = 4;

pub const RMPP_WIDTH: u32 = 954;
pub const RMPP_HEIGHT: u32 = 1696;

// QTFB input event constants (for input.rs)
pub const MESSAGE_USERINPUT: u8 = 4;
pub const INPUT_TOUCH_PRESS: i32 = 0x10;
pub const INPUT_TOUCH_RELEASE: i32 = 0x11;
pub const INPUT_TOUCH_UPDATE: i32 = 0x12;

// ---- repr(C) message structs matching QTFB C++ layout ----

#[repr(C)]
#[derive(Clone, Copy)]
struct InitContents {
    framebuffer_key: u32,
    framebuffer_type: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UpdateContents {
    msg_type: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

#[repr(C)]
union ClientMessageBody {
    init: InitContents,
    update: UpdateContents,
    refresh_mode: i32,
}

#[repr(C)]
struct ClientMessage {
    msg_type: u8,
    body: ClientMessageBody,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct InitResponseBody {
    shm_key_defined: i32,
    shm_size: usize,
}

#[repr(C)]
struct ServerMessage {
    msg_type: u8,
    init: InitResponseBody,
}

/// QTFB shared-memory display backend for reMarkable Paper Pro.
pub struct QtfbDisplay {
    fd: RawFd,
    buffer: *mut u8,
    buffer_size: usize,
    width: u32,
    height: u32,
    bpp: u32,
    font_regular: fontdue::Font,
}

unsafe impl Send for QtfbDisplay {}

impl QtfbDisplay {
    pub fn new() -> Self {
        let fb_key = std::env::var("QTFB_KEY")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(DEFAULT_FB_KEY);

        log::info!("Connecting to QTFB socket with key {}", fb_key);

        let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET, 0) };
        if fd < 0 {
            panic!(
                "Failed to create QTFB socket: {}",
                std::io::Error::last_os_error()
            );
        }

        let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        unsafe {
            std::ptr::copy_nonoverlapping(
                SOCKET_PATH.as_ptr(),
                addr.sun_path.as_mut_ptr() as *mut u8,
                SOCKET_PATH.len(),
            );
        }

        let ret = unsafe {
            libc::connect(
                fd,
                &addr as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t,
            )
        };
        if ret != 0 {
            panic!(
                "Failed to connect to QTFB at /tmp/qtfb.sock: {}. Is AppLoad installed and xochitl running?",
                std::io::Error::last_os_error()
            );
        }
        log::info!("Connected to QTFB socket");

        let init_msg = ClientMessage {
            msg_type: MESSAGE_INITIALIZE,
            body: ClientMessageBody {
                init: InitContents {
                    framebuffer_key: fb_key,
                    framebuffer_type: FBFMT_RMPP_RGB888,
                },
            },
        };

        let sent = unsafe {
            libc::send(
                fd,
                &init_msg as *const _ as *const libc::c_void,
                std::mem::size_of::<ClientMessage>(),
                0,
            )
        };
        if sent < 0 {
            panic!(
                "Failed to send QTFB init: {}",
                std::io::Error::last_os_error()
            );
        }

        let mut server_msg: ServerMessage = unsafe { std::mem::zeroed() };
        let received = unsafe {
            libc::recv(
                fd,
                &mut server_msg as *mut _ as *mut libc::c_void,
                std::mem::size_of::<ServerMessage>(),
                0,
            )
        };
        if received < 1 {
            panic!(
                "Failed to receive QTFB init response: {}",
                std::io::Error::last_os_error()
            );
        }

        let shm_key = server_msg.init.shm_key_defined;
        let shm_size = server_msg.init.shm_size;
        log::info!(
            "QTFB SHM key: {}, size: {} bytes ({}x{} RGB888 = {})",
            shm_key,
            shm_size,
            RMPP_WIDTH,
            RMPP_HEIGHT,
            RMPP_WIDTH as usize * RMPP_HEIGHT as usize * 3
        );

        let shm_path = format!("/dev/shm/qtfb_{}", shm_key);
        let shm_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&shm_path)
            .unwrap_or_else(|e| panic!("Failed to open SHM at {}: {}", shm_path, e));

        use std::os::unix::io::AsRawFd;
        let shm_fd = shm_file.as_raw_fd();

        let shm_ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                shm_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                shm_fd,
                0,
            )
        };
        if shm_ptr == libc::MAP_FAILED {
            panic!(
                "Failed to mmap QTFB SHM: {}",
                std::io::Error::last_os_error()
            );
        }

        std::mem::forget(shm_file);

        let font_regular =
            fontdue::Font::from_bytes(FONT_REGULAR, fontdue::FontSettings::default())
                .expect("Failed to load regular font");

        let mode_msg = ClientMessage {
            msg_type: MESSAGE_SET_REFRESH_MODE,
            body: ClientMessageBody {
                refresh_mode: REFRESH_MODE_CONTENT,
            },
        };
        unsafe {
            libc::send(
                fd,
                &mode_msg as *const _ as *const libc::c_void,
                std::mem::size_of::<ClientMessage>(),
                0,
            );
        }

        let mut display = QtfbDisplay {
            fd,
            buffer: shm_ptr as *mut u8,
            buffer_size: shm_size,
            width: RMPP_WIDTH,
            height: RMPP_HEIGHT,
            bpp: 3,
            font_regular,
        };

        display.clear();
        display.full_refresh();
        std::thread::sleep(std::time::Duration::from_millis(500));

        log::info!(
            "QTFB display initialized: {}x{} RGB888",
            RMPP_WIDTH,
            RMPP_HEIGHT
        );
        display
    }

    pub fn socket_fd(&self) -> RawFd {
        self.fd
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    fn buffer_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.buffer, self.buffer_size) }
    }

    fn buffer_ref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.buffer, self.buffer_size) }
    }

    #[inline]
    fn stride(&self) -> u32 {
        self.width * self.bpp
    }

    #[inline]
    fn set_pixel(&mut self, x: u32, y: u32, c: color) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = (y * self.stride() + x * self.bpp) as usize;
        let buf = self.buffer_mut();
        if offset + 2 < buf.len() {
            buf[offset] = c.r;
            buf[offset + 1] = c.g;
            buf[offset + 2] = c.b;
        }
    }

    #[inline]
    fn get_pixel(&self, x: u32, y: u32) -> color {
        if x >= self.width || y >= self.height {
            return color::WHITE;
        }
        let offset = (y * self.stride() + x * self.bpp) as usize;
        let buf = self.buffer_ref();
        if offset + 2 < buf.len() {
            color {
                r: buf[offset],
                g: buf[offset + 1],
                b: buf[offset + 2],
            }
        } else {
            color::WHITE
        }
    }

    pub fn clear(&mut self) {
        let fill_len = self.width as usize * self.height as usize * self.bpp as usize;
        let buf = self.buffer_mut();
        buf[..fill_len].fill(0xFF);
    }

    pub fn fill_rect(&mut self, pos: Point2<i32>, size: cgmath::Vector2<u32>, c: color) {
        let x0 = pos.x.max(0) as u32;
        let y0 = pos.y.max(0) as u32;
        let x1 = ((pos.x.max(0) as u32) + size.x).min(self.width);
        let y1 = ((pos.y.max(0) as u32) + size.y).min(self.height);
        let bpp = self.bpp;
        let stride = self.stride();

        let buf = self.buffer_mut();
        for y in y0..y1 {
            let row_start = (y * stride + x0 * bpp) as usize;
            let row_end = (y * stride + x1 * bpp) as usize;
            if row_end <= buf.len() {
                for i in (row_start..row_end).step_by(3) {
                    buf[i] = c.r;
                    buf[i + 1] = c.g;
                    buf[i + 2] = c.b;
                }
            }
        }
    }

    pub fn draw_line(&mut self, from: Point2<i32>, to: Point2<i32>, width: u32, c: color) {
        let dx = (to.x - from.x).abs();
        let dy = -(to.y - from.y).abs();
        let sx: i32 = if from.x < to.x { 1 } else { -1 };
        let sy: i32 = if from.y < to.y { 1 } else { -1 };
        let mut err = dx + dy;
        let mut x = from.x;
        let mut y = from.y;
        let half_w = width as i32 / 2;

        loop {
            for dy2 in -half_w..=(half_w) {
                for dx2 in -half_w..=(half_w) {
                    self.set_pixel((x + dx2) as u32, (y + dy2) as u32, c);
                }
            }
            if x == to.x && y == to.y {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    pub fn draw_rect(
        &mut self,
        pos: Point2<i32>,
        size: cgmath::Vector2<u32>,
        border_px: u32,
        c: color,
    ) {
        let x = pos.x;
        let y = pos.y;
        let w = size.x as i32;
        let h = size.y as i32;

        self.fill_rect(
            Point2 { x, y },
            cgmath::Vector2 {
                x: size.x,
                y: border_px,
            },
            c,
        );
        self.fill_rect(
            Point2 {
                x,
                y: y + h - border_px as i32,
            },
            cgmath::Vector2 {
                x: size.x,
                y: border_px,
            },
            c,
        );
        self.fill_rect(
            Point2 { x, y },
            cgmath::Vector2 {
                x: border_px,
                y: size.y,
            },
            c,
        );
        self.fill_rect(
            Point2 {
                x: x + w - border_px as i32,
                y,
            },
            cgmath::Vector2 {
                x: border_px,
                y: size.y,
            },
            c,
        );
    }

    pub fn draw_image(&mut self, img: &RgbImage, pos: Point2<i32>) {
        let img_w = img.width();
        let img_h = img.height();

        for iy in 0..img_h {
            let dy = pos.y + iy as i32;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            for ix in 0..img_w {
                let dx = pos.x + ix as i32;
                if dx < 0 || dx >= self.width as i32 {
                    continue;
                }
                let pixel = img.get_pixel(ix, iy);
                self.set_pixel(
                    dx as u32,
                    dy as u32,
                    color {
                        r: pixel[0],
                        g: pixel[1],
                        b: pixel[2],
                    },
                );
            }
        }
    }

    pub fn dump_region(&self, rect: mxcfb_rect) -> Option<Vec<u8>> {
        let mut data = Vec::with_capacity((rect.width * rect.height * 3) as usize);
        for y in rect.top..(rect.top + rect.height) {
            for x in rect.left..(rect.left + rect.width) {
                let c = self.get_pixel(x, y);
                data.push(c.r);
                data.push(c.g);
                data.push(c.b);
            }
        }
        Some(data)
    }

    pub fn draw_text(
        &mut self,
        pos: Point2<f32>,
        text: &str,
        size: f32,
        c: color,
        dryrun: bool,
    ) -> mxcfb_rect {
        let mut x_offset = 0.0f32;
        let mut max_height = 0u32;
        let mut glyphs: Vec<(fontdue::Metrics, Vec<u8>, f32)> = Vec::new();

        for ch in text.chars() {
            let (metrics, bitmap) = self.font_regular.rasterize(ch, size);
            let glyph_x = x_offset;
            x_offset += metrics.advance_width;
            max_height = max_height.max(metrics.height as u32);
            glyphs.push((metrics, bitmap, glyph_x));
        }

        let total_width = x_offset.ceil() as u32;
        let total_height = (size * 1.2) as u32;

        let rect = mxcfb_rect {
            left: pos.x as u32,
            top: pos.y.max(0.0) as u32,
            width: total_width,
            height: total_height,
        };

        if dryrun {
            return rect;
        }

        let baseline_y = pos.y + size * 0.8;

        for (metrics, bitmap, glyph_x) in &glyphs {
            let gx = pos.x + glyph_x + metrics.xmin as f32;
            let gy = baseline_y - metrics.height as f32 - metrics.ymin as f32;

            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let alpha = bitmap[row * metrics.width + col];
                    if alpha == 0 {
                        continue;
                    }
                    let px = (gx + col as f32) as i32;
                    let py = (gy + row as f32) as i32;
                    if px < 0
                        || py < 0
                        || px >= self.width as i32
                        || py >= self.height as i32
                    {
                        continue;
                    }

                    let bg = self.get_pixel(px as u32, py as u32);
                    let a = alpha as f32 / 255.0;
                    let inv_a = 1.0 - a;
                    let blended = color {
                        r: (c.r as f32 * a + bg.r as f32 * inv_a) as u8,
                        g: (c.g as f32 * a + bg.g as f32 * inv_a) as u8,
                        b: (c.b as f32 * a + bg.b as f32 * inv_a) as u8,
                    };
                    self.set_pixel(px as u32, py as u32, blended);
                }
            }
        }

        rect
    }

    /// Draw a filled circle at center (cx, cy) with given radius and color.
    pub fn fill_circle(&mut self, cx: i32, cy: i32, radius: i32, c: color) {
        let r2 = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= r2 {
                    self.set_pixel((cx + dx) as u32, (cy + dy) as u32, c);
                }
            }
        }
    }

    fn send_msg(&self, msg: &ClientMessage) {
        let ret = unsafe {
            libc::send(
                self.fd,
                msg as *const _ as *const libc::c_void,
                std::mem::size_of::<ClientMessage>(),
                0,
            )
        };
        if ret < 0 {
            log::warn!(
                "QTFB send failed: {}",
                std::io::Error::last_os_error()
            );
        }
    }

    pub fn full_refresh(&self) -> u32 {
        let msg = ClientMessage {
            msg_type: MESSAGE_REQUEST_FULL_REFRESH,
            body: ClientMessageBody { refresh_mode: 0 },
        };
        self.send_msg(&msg);

        let update_msg = ClientMessage {
            msg_type: MESSAGE_UPDATE,
            body: ClientMessageBody {
                update: UpdateContents {
                    msg_type: UPDATE_ALL,
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 0,
                },
            },
        };
        self.send_msg(&update_msg);
        0
    }

    pub fn partial_refresh(&self, region: &mxcfb_rect) -> u32 {
        let msg = ClientMessage {
            msg_type: MESSAGE_UPDATE,
            body: ClientMessageBody {
                update: UpdateContents {
                    msg_type: UPDATE_PARTIAL,
                    x: region.left as i32,
                    y: region.top as i32,
                    w: region.width as i32,
                    h: region.height as i32,
                },
            },
        };
        self.send_msg(&msg);
        0
    }

    pub fn set_refresh_mode(&self, fast: bool) {
        let mode = if fast {
            REFRESH_MODE_FAST
        } else {
            REFRESH_MODE_CONTENT
        };
        let msg = ClientMessage {
            msg_type: MESSAGE_SET_REFRESH_MODE,
            body: ClientMessageBody {
                refresh_mode: mode,
            },
        };
        self.send_msg(&msg);
    }
}

impl Drop for QtfbDisplay {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.buffer as *mut libc::c_void, self.buffer_size);
            libc::close(self.fd);
        }
    }
}
