use crate::activity::SlidingWindowProcessor;
use crate::smoothing::{interpolate_to_frames, SmoothingPreset, TrajectorySmoother, ZoomMode};
use crate::types::{MemoryEfficientFrame, ProcessingOptions, TrajectoryPoint, WindowTarget};
use crate::utils::{log_str, rgba_to_grayscale, downsample_grayscale, Timer};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

/// Main video processor that coordinates the entire pipeline
pub struct VideoProcessor {
    options: ProcessingOptions,
}

impl VideoProcessor {
    pub fn new(options: ProcessingOptions) -> Self {
        VideoProcessor { options }
    }

    /// Process frames in memory-efficient way
    /// Returns activity targets for the entire video
    pub fn analyze_frames(
        &self,
        frames: Vec<MemoryEfficientFrame>,
        fps: f64,
    ) -> Result<Vec<WindowTarget>, JsValue> {
        let _timer = Timer::new("Activity Analysis");

        log_str(&format!(
            "Analyzing {} frames at {:.1} fps",
            frames.len(),
            fps
        ));

        let processor = SlidingWindowProcessor::new(
            self.options.window_seconds,
            fps,
            self.options.threshold,
        );

        let targets = processor.process_frames(&frames, fps);

        log_str(&format!("Generated {} activity targets", targets.len()));

        Ok(targets)
    }

    /// Generate smooth trajectory from targets
    pub fn generate_trajectory(
        &self,
        targets: &[WindowTarget],
        fps: f64,
        frame_count: usize,
        src_w: u32,
        src_h: u32,
    ) -> Result<Vec<TrajectoryPoint>, JsValue> {
        let _timer = Timer::new("Trajectory Generation");

        log_str("Interpolating trajectory to per-frame...");

        // Interpolate to per-frame
        let preset = SmoothingPreset::from_str(&self.options.smoothing_preset);
        let zoom_mode = ZoomMode::from_str(&self.options.zoom_mode);

        let mut trajectory = interpolate_to_frames(
            targets,
            fps,
            frame_count,
            src_w,
            src_h,
            self.options.out_width,
            self.options.out_height,
            zoom_mode,
            self.options.zoom_max,
            self.options.padding,
        );

        log_str(&format!(
            "Interpolated to {} trajectory points",
            trajectory.len()
        ));

        // Apply smoothing
        log_str(&format!("Applying {:?} smoothing...", preset));

        let smoother = TrajectorySmoother::new(
            preset,
            self.options.smooth_window,
            self.options.smooth_strength,
        );

        trajectory = smoother.smooth_trajectory(&trajectory, fps);

        log_str("Trajectory smoothing complete");

        Ok(trajectory)
    }

    /// Convert RGBA ImageData to memory-efficient grayscale frame
    /// Optionally downsample for memory efficiency
    pub fn process_frame_data(
        &self,
        image_data: &ImageData,
        downsample_to_height: Option<u32>,
    ) -> Result<MemoryEfficientFrame, JsValue> {
        let width = image_data.width();
        let height = image_data.height();
        let rgba_data = image_data.data();

        // Convert to grayscale
        let gray_data = rgba_to_grayscale(&rgba_data, width, height);

        // Optionally downsample
        if let Some(target_h) = downsample_to_height {
            if target_h < height {
                let (downsampled, new_w, new_h) =
                    downsample_grayscale(&gray_data, width, height, target_h);

                return Ok(MemoryEfficientFrame {
                    grayscale_data: downsampled,
                    width: new_w,
                    height: new_h,
                });
            }
        }

        Ok(MemoryEfficientFrame {
            grayscale_data: gray_data,
            width,
            height,
        })
    }

    /// Crop and resize a single frame based on trajectory point
    pub fn crop_frame(
        &self,
        canvas: &HtmlCanvasElement,
        ctx: &CanvasRenderingContext2d,
        source_canvas: &HtmlCanvasElement,
        trajectory_point: &TrajectoryPoint,
        src_w: u32,
        src_h: u32,
        border_pct: f64,
    ) -> Result<(), JsValue> {
        let border_x = (src_w as f64 * border_pct / 100.0) as i32;
        let border_y = (src_h as f64 * border_pct / 100.0) as i32;
        let canvas_w = src_w as i32 + 2 * border_x;
        let canvas_h = src_h as i32 + 2 * border_y;

        // Adjust trajectory coords for border
        let cx = trajectory_point.x + border_x as f64;
        let cy = trajectory_point.y + border_y as f64;

        // Compute crop box
        let (x, y, crop_w, crop_h) = crate::utils::compute_crop_box(
            cx,
            cy,
            canvas_w as u32,
            canvas_h as u32,
            self.options.out_width,
            self.options.out_height,
            self.options.padding,
            trajectory_point.zoom,
        );

        // Clear output canvas
        canvas.set_width(self.options.out_width);
        canvas.set_height(self.options.out_height);
        ctx.clear_rect(0.0, 0.0, self.options.out_width as f64, self.options.out_height as f64);

        // Draw cropped and scaled region
        ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
            source_canvas,
            x as f64,
            y as f64,
            crop_w as f64,
            crop_h as f64,
            0.0,
            0.0,
            self.options.out_width as f64,
            self.options.out_height as f64,
        )?;

        Ok(())
    }
}

/// Helper to create a canvas element
pub fn create_canvas(width: u32, height: u32) -> Result<(HtmlCanvasElement, CanvasRenderingContext2d), JsValue> {
    let document = web_sys::window()
        .ok_or("No window")?
        .document()
        .ok_or("No document")?;

    let canvas = document
        .create_element("canvas")?
        .dyn_into::<HtmlCanvasElement>()?;

    canvas.set_width(width);
    canvas.set_height(height);

    let ctx = canvas
        .get_context("2d")?
        .ok_or("Failed to get 2d context")?
        .dyn_into::<CanvasRenderingContext2d>()?;

    Ok((canvas, ctx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_processor_creation() {
        let options = ProcessingOptions::default();
        let processor = VideoProcessor::new(options);
        assert_eq!(processor.options.out_width, 1080);
    }
}
