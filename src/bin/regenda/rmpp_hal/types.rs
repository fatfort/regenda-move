pub use cgmath::{vec2, Point2, Vector2};
pub use image;

/// Which reMarkable device the binary is currently running on. Detected once
/// at startup from `/sys/devices/soc0/machine`, the device-tree model, and
/// `gethostname()`. Determines display dimensions, the QTFB INIT path, and
/// the UI scale factor.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    /// reMarkable Paper Pro (11.8", 1620×2160). Hostname `imx8mm-ferrari`.
    Ferrari,
    /// reMarkable Paper Pro Move (7.3", 954×1696). Hostname `imx93-chiappa`.
    Move,
}

impl DeviceKind {
    pub fn dims(self) -> (u32, u32) {
        match self {
            DeviceKind::Ferrari => (1620, 2160),
            DeviceKind::Move => (954, 1696),
        }
    }

    /// UI scale factor relative to the rMPP-native (Ferrari) sizes. Move's
    /// physical screen is 7.3"/11.8" the diagonal of Ferrari's, so we scale
    /// every native pixel size by that ratio to keep the UI proportional.
    pub fn ui_scale(self) -> f32 {
        match self {
            DeviceKind::Ferrari => 1.0,
            DeviceKind::Move => 7.3 / 11.8,
        }
    }
}

/// Runtime display info populated once `QtfbDisplay` has detected and
/// initialised the device.
#[derive(Copy, Clone, Debug)]
pub struct DisplayInfo {
    pub width: u32,
    pub height: u32,
    pub device: DeviceKind,
    pub ui_scale: f32,
}

/// Rectangle region on display (mirrors libremarkable's mxcfb_rect).
#[derive(Copy, Clone, Debug, Default)]
pub struct mxcfb_rect {
    pub top: u32,
    pub left: u32,
    pub width: u32,
    pub height: u32,
}

impl mxcfb_rect {
    pub fn size(&self) -> Vector2<u32> {
        Vector2 {
            x: self.width,
            y: self.height,
        }
    }

    pub fn top_left(&self) -> Point2<u32> {
        Point2 {
            x: self.left,
            y: self.top,
        }
    }
}

/// RGB color. The RPP Gallery 3 e-ink display supports color via QTFB RGB888.
#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[allow(non_upper_case_globals)]
impl color {
    pub const BLACK: color = color { r: 0, g: 0, b: 0 };
    pub const WHITE: color = color {
        r: 255,
        g: 255,
        b: 255,
    };

    pub fn GRAY(v: u8) -> color {
        color { r: v, g: v, b: v }
    }

    // Calendar UI colors
    pub const HEADER_BG: color = color { r: 50, g: 50, b: 50 };
    pub const ACCENT: color = color {
        r: 0,
        g: 100,
        b: 200,
    };
    pub const LIGHT_GRAY: color = color {
        r: 220,
        g: 220,
        b: 220,
    };
    pub const MEDIUM_GRAY: color = color {
        r: 160,
        g: 160,
        b: 160,
    };
    pub const DARK_GRAY: color = color {
        r: 100,
        g: 100,
        b: 100,
    };
    pub const TODAY_BG: color = color {
        r: 230,
        g: 240,
        b: 255,
    };
}

/// Input event from touch or buttons.
#[derive(Clone, Debug)]
pub enum InputEvent {
    MultitouchEvent { event: MultitouchEvent },
    #[allow(dead_code)]
    GPIO { event: GPIOEvent },
}

/// Multitouch event types.
#[derive(Clone, Debug)]
pub enum MultitouchEvent {
    Press { finger: Finger },
    Release { finger: Finger },
    Move { finger: Finger },
}

/// A finger/touch point.
#[derive(Clone, Debug)]
pub struct Finger {
    pub pos: Point2<u16>,
    pub tracking_id: i32,
}

/// GPIO button event (RPP has no physical page buttons, kept for API compat).
#[derive(Clone, Debug)]
pub enum GPIOEvent {
    Press { button: PhysicalButton },
    #[allow(dead_code)]
    Release { button: PhysicalButton },
}

/// Physical button identifiers.
#[derive(Clone, Debug)]
pub enum PhysicalButton {
    LEFT,
    #[allow(dead_code)]
    MIDDLE,
    RIGHT,
    #[allow(dead_code)]
    POWER,
}
