mod activity;
mod smoothing;
mod types;
mod utils;
mod video_processor;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, ImageData};

pub use types::{ProcessingOptions, TrajectoryPoint, WindowTarget};
pub use video_processor::VideoProcessor;

/// Initialize panic hook for better error messages in console
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();

    utils::log_str("FitVid WASM initialized");
}

/// JavaScript-friendly wrapper for the video processor
#[wasm_bindgen]
pub struct FitvidProcessor {
    processor: VideoProcessor,
    frames: Vec<types::MemoryEfficientFrame>,
    targets: Vec<WindowTarget>,
    trajectory: Vec<TrajectoryPoint>,
    fps: f64,
    src_width: u32,
    src_height: u32,
}

#[wasm_bindgen]
impl FitvidProcessor {
    /// Create a new processor with options
    #[wasm_bindgen(constructor)]
    pub fn new(options_js: JsValue) -> Result<FitvidProcessor, JsValue> {
        let options: ProcessingOptions = serde_wasm_bindgen::from_value(options_js)
            .unwrap_or_else(|_| ProcessingOptions::default());

        utils::log_str(&format!(
            "Creating FitvidProcessor: {}x{} output",
            options.out_width, options.out_height
        ));

        Ok(FitvidProcessor {
            processor: VideoProcessor::new(options),
            frames: Vec::new(),
            targets: Vec::new(),
            trajectory: Vec::new(),
            fps: 30.0,
            src_width: 0,
            src_height: 0,
        })
    }

    /// Set video metadata
    #[wasm_bindgen]
    pub fn set_video_metadata(&mut self, width: u32, height: u32, fps: f64) {
        self.src_width = width;
        self.src_height = height;
        self.fps = fps;

        utils::log_str(&format!(
            "Video metadata: {}x{} @ {:.1} fps",
            width, height, fps
        ));
    }

    /// Set analysis scale factor (if frames were downsampled)
    #[wasm_bindgen]
    pub fn set_analysis_scale(&mut self, analysis_width: u32, analysis_height: u32) {
        let scale_x = self.src_width as f64 / analysis_width as f64;
        let scale_y = self.src_height as f64 / analysis_height as f64;

        // Scale up all target coordinates
        for target in &mut self.targets {
            target.cx *= scale_x;
            target.cy *= scale_y;
            target.spread *= scale_x.max(scale_y);
            target.bbox_w *= scale_x;
            target.bbox_h *= scale_y;
        }

        utils::log_str(&format!(
            "Scaled targets from {}x{} to {}x{} (scale: {:.2}x, {:.2}x)",
            analysis_width, analysis_height,
            self.src_width, self.src_height,
            scale_x, scale_y
        ));
    }

    /// Add a frame for analysis (pass ImageData from canvas)
    /// downsample_height: optional target height for downsampling (e.g., 720)
    #[wasm_bindgen]
    pub fn add_frame(
        &mut self,
        image_data: ImageData,
        downsample_height: Option<u32>,
    ) -> Result<(), JsValue> {
        let frame = self
            .processor
            .process_frame_data(&image_data, downsample_height)?;

        self.frames.push(frame);

        Ok(())
    }

    /// Get current frame count
    #[wasm_bindgen]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Analyze all collected frames and generate activity targets
    #[wasm_bindgen]
    pub fn analyze(&mut self) -> Result<usize, JsValue> {
        if self.frames.is_empty() {
            return Err(JsValue::from_str("No frames to analyze"));
        }

        utils::log_str("Starting activity analysis...");

        self.targets = self.processor.analyze_frames(
            std::mem::take(&mut self.frames), // Take frames to avoid cloning
            self.fps,
        )?;

        // Debug: Log all targets to see variation
        utils::log_str(&format!("Activity targets (total: {}):", self.targets.len()));
        for (i, target) in self.targets.iter().enumerate() {
            utils::log_str(&format!(
                "  Target {}: time={:.1}s, cx={:.1}, cy={:.1}, spread={:.1}, bbox={}x{}",
                i, target.timestamp, target.cx, target.cy, target.spread,
                target.bbox_w as i32, target.bbox_h as i32
            ));
        }

        Ok(self.targets.len())
    }

    /// Generate smooth trajectory from targets
    #[wasm_bindgen]
    pub fn generate_trajectory(&mut self, frame_count: usize) -> Result<usize, JsValue> {
        if self.targets.is_empty() {
            return Err(JsValue::from_str("No targets available. Run analyze() first."));
        }

        utils::log_str("Generating smooth trajectory...");

        self.trajectory = self.processor.generate_trajectory(
            &self.targets,
            self.fps,
            frame_count,
            self.src_width,
            self.src_height,
        )?;

        // Debug: Log first few trajectory points
        if self.trajectory.len() > 0 {
            utils::log_str(&format!(
                "Trajectory sample - Frame 0: x={:.1}, y={:.1}, zoom={:.2}",
                self.trajectory[0].x, self.trajectory[0].y, self.trajectory[0].zoom
            ));
            if self.trajectory.len() > self.trajectory.len() / 2 {
                let mid = self.trajectory.len() / 2;
                utils::log_str(&format!(
                    "Trajectory sample - Frame {}: x={:.1}, y={:.1}, zoom={:.2}",
                    mid, self.trajectory[mid].x, self.trajectory[mid].y, self.trajectory[mid].zoom
                ));
            }
        }

        Ok(self.trajectory.len())
    }

    /// Get trajectory point for a specific frame
    #[wasm_bindgen]
    pub fn get_trajectory_point(&self, frame_idx: usize) -> JsValue {
        if frame_idx >= self.trajectory.len() {
            return JsValue::NULL;
        }

        let point = &self.trajectory[frame_idx];
        let obj = js_sys::Object::new();

        js_sys::Reflect::set(&obj, &"x".into(), &point.x.into()).unwrap();
        js_sys::Reflect::set(&obj, &"y".into(), &point.y.into()).unwrap();
        js_sys::Reflect::set(&obj, &"zoom".into(), &point.zoom.into()).unwrap();

        obj.into()
    }

    /// Get all trajectory points as JSON
    #[wasm_bindgen]
    pub fn get_trajectory_json(&self) -> Result<String, JsValue> {
        let points: Vec<_> = self
            .trajectory
            .iter()
            .map(|p| serde_json::json!({ "x": p.x, "y": p.y, "zoom": p.zoom }))
            .collect();

        serde_json::to_string(&points)
            .map_err(|e| JsValue::from_str(&format!("JSON error: {}", e)))
    }

    /// Crop a frame using the computed trajectory
    /// frame_idx: the frame number to process
    /// source_canvas: canvas containing the source frame
    /// output_canvas: canvas to draw the cropped result
    #[wasm_bindgen]
    pub fn crop_frame(
        &self,
        frame_idx: usize,
        source_canvas: HtmlCanvasElement,
        output_canvas: HtmlCanvasElement,
        border_pct: f64,
    ) -> Result<(), JsValue> {
        if frame_idx >= self.trajectory.len() {
            return Err(JsValue::from_str("Frame index out of bounds"));
        }

        let ctx = output_canvas
            .get_context("2d")?
            .ok_or("Failed to get 2d context")?
            .dyn_into::<web_sys::CanvasRenderingContext2d>()?;

        let point = &self.trajectory[frame_idx];

        self.processor.crop_frame(
            &output_canvas,
            &ctx,
            &source_canvas,
            point,
            self.src_width,
            self.src_height,
            border_pct,
        )?;

        Ok(())
    }

    /// Clear all stored data
    #[wasm_bindgen]
    pub fn clear(&mut self) {
        self.frames.clear();
        self.targets.clear();
        self.trajectory.clear();
        utils::log_str("Processor cleared");
    }

    /// Get memory usage estimate in MB
    #[wasm_bindgen]
    pub fn memory_estimate_mb(&self) -> f64 {
        let frame_bytes: usize = self
            .frames
            .iter()
            .map(|f| f.grayscale_data.len())
            .sum();

        let target_bytes = self.targets.len() * std::mem::size_of::<WindowTarget>();
        let trajectory_bytes = self.trajectory.len() * std::mem::size_of::<TrajectoryPoint>();

        let total_bytes = frame_bytes + target_bytes + trajectory_bytes;
        total_bytes as f64 / (1024.0 * 1024.0)
    }
}

/// Standalone utility functions

/// Compute crop box for a given center point and zoom
#[wasm_bindgen]
pub fn compute_crop_box(
    cx: f64,
    cy: f64,
    src_w: u32,
    src_h: u32,
    out_w: u32,
    out_h: u32,
    padding: u32,
    zoom: f64,
) -> js_sys::Array {
    let (x, y, w, h) = utils::compute_crop_box(cx, cy, src_w, src_h, out_w, out_h, padding, zoom);

    let arr = js_sys::Array::new();
    arr.push(&(x as u32).into());
    arr.push(&(y as u32).into());
    arr.push(&(w as u32).into());
    arr.push(&(h as u32).into());
    arr
}

/// Get platform presets
#[wasm_bindgen]
pub fn get_platform_preset(platform: &str) -> JsValue {
    let (width, height) = match platform.to_lowercase().as_str() {
        "tiktok" | "reels" | "ig-story" | "yt-shorts" => (1080, 1920),
        "ig-post" | "fb-feed" => (1080, 1350),
        "ig-square" | "linkedin" => (1080, 1080),
        _ => (1080, 1920), // default to vertical
    };

    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"width".into(), &width.into()).unwrap();
    js_sys::Reflect::set(&obj, &"height".into(), &height.into()).unwrap();

    obj.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn test_init() {
        init();
        // If we get here without panicking, initialization worked
    }

    #[wasm_bindgen_test]
    fn test_platform_preset() {
        let preset = get_platform_preset("tiktok");
        assert!(!preset.is_null());
    }
}
