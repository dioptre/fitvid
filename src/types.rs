use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// Activity target for a single analysis window.
/// Represents where activity is concentrated during a time window.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[wasm_bindgen]
pub struct WindowTarget {
    /// Center of window in seconds
    pub timestamp: f64,
    /// Center x (pixels, in source frame coords)
    pub cx: f64,
    /// Center y (pixels, in source frame coords)
    pub cy: f64,
    /// Activity spread (std dev of active pixels)
    pub spread: f64,
    /// Bounding box width of active region
    pub bbox_w: f64,
    /// Bounding box height of active region
    pub bbox_h: f64,
}

#[wasm_bindgen]
impl WindowTarget {
    #[wasm_bindgen(constructor)]
    pub fn new(
        timestamp: f64,
        cx: f64,
        cy: f64,
        spread: f64,
        bbox_w: f64,
        bbox_h: f64,
    ) -> WindowTarget {
        WindowTarget {
            timestamp,
            cx,
            cy,
            spread,
            bbox_w,
            bbox_h,
        }
    }
}

/// Processing options for the video
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingOptions {
    /// Output width
    pub out_width: u32,
    /// Output height
    pub out_height: u32,
    /// Analysis window size in seconds
    pub window_seconds: f64,
    /// Activity threshold (0-100)
    pub threshold: u8,
    /// Smoothing preset: "default", "snappy", or "cinematic"
    pub smoothing_preset: String,
    /// Smoothing strength (0.0-1.0)
    pub smooth_strength: f64,
    /// Smoothing window (seconds)
    pub smooth_window: f64,
    /// Zoom mode: "auto", "none", or "close"
    pub zoom_mode: String,
    /// Maximum zoom factor for auto mode
    pub zoom_max: f64,
    /// Extra pixels around detected region
    pub padding: u32,
    /// Black border as % of frame size
    pub border_pct: f64,
}

impl Default for ProcessingOptions {
    fn default() -> Self {
        ProcessingOptions {
            out_width: 1080,
            out_height: 1920,
            window_seconds: 2.0,
            threshold: 10,
            smoothing_preset: "default".to_string(),
            smooth_strength: 0.5,
            smooth_window: 2.0,
            zoom_mode: "auto".to_string(),
            zoom_max: 2.0,
            padding: 50,
            border_pct: 5.0,
        }
    }
}

/// Video metadata extracted from the source
#[derive(Debug, Clone)]
pub struct VideoMetadata {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub frame_count: usize,
    pub duration: f64,
}

/// A lightweight frame representation for analysis
/// Stores grayscale data and optionally downsampled
#[derive(Debug, Clone)]
pub struct MemoryEfficientFrame {
    pub grayscale_data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl MemoryEfficientFrame {
    pub fn new(width: u32, height: u32) -> Self {
        let size = (width * height) as usize;
        MemoryEfficientFrame {
            grayscale_data: vec![0u8; size],
            width,
            height,
        }
    }

    pub fn get_pixel(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        let idx = (y * self.width + x) as usize;
        self.grayscale_data[idx]
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, value: u8) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = (y * self.width + x) as usize;
        self.grayscale_data[idx] = value;
    }
}

/// Trajectory point: (x, y, zoom)
#[derive(Debug, Clone, Copy)]
pub struct TrajectoryPoint {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl TrajectoryPoint {
    pub fn new(x: f64, y: f64, zoom: f64) -> Self {
        TrajectoryPoint { x, y, zoom }
    }
}
