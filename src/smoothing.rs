use crate::types::{TrajectoryPoint, WindowTarget};
use nalgebra::{DMatrix, DVector};

/// Smoothing preset options
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SmoothingPreset {
    Default,
    Snappy,
    Cinematic,
}

impl SmoothingPreset {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "snappy" => SmoothingPreset::Snappy,
            "cinematic" => SmoothingPreset::Cinematic,
            _ => SmoothingPreset::Default,
        }
    }
}

/// Zoom mode for determining zoom levels
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoomMode {
    Auto,
    None,
    Close,
}

impl ZoomMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "auto" => ZoomMode::Auto,
            "close" => ZoomMode::Close,
            _ => ZoomMode::None,
        }
    }
}

/// Trajectory smoother that applies various smoothing algorithms
pub struct TrajectorySmoother {
    preset: SmoothingPreset,
    smooth_window: f64,
    smooth_strength: f64,
}

impl TrajectorySmoother {
    pub fn new(preset: SmoothingPreset, smooth_window: f64, smooth_strength: f64) -> Self {
        TrajectorySmoother {
            preset,
            smooth_window,
            smooth_strength,
        }
    }

    /// Main smoothing pipeline
    pub fn smooth_trajectory(
        &self,
        raw_trajectory: &[TrajectoryPoint],
        fps: f64,
    ) -> Vec<TrajectoryPoint> {
        if raw_trajectory.is_empty() {
            return Vec::new();
        }

        let sigma = self.smooth_window * fps * self.smooth_strength;

        // Separate x, y, zoom for processing
        let mut xy: Vec<(f64, f64)> = raw_trajectory
            .iter()
            .map(|p| (p.x, p.y))
            .collect();

        let mut zoom: Vec<f64> = raw_trajectory
            .iter()
            .map(|p| p.zoom)
            .collect();

        match self.preset {
            SmoothingPreset::Snappy => {
                let alpha = 0.15 + 0.35 * self.smooth_strength;
                xy = Self::smooth_ema_2d(&xy, alpha);
                zoom = Self::smooth_ema_1d(&zoom, alpha);
            }
            SmoothingPreset::Cinematic => {
                xy = Self::smooth_kalman(&xy);
                xy = Self::smooth_gaussian_2d(&xy, sigma);
                xy = Self::smooth_ease_in_out(&xy, 50.0, (fps * 0.8) as usize);
                zoom = Self::smooth_gaussian_1d(&zoom, sigma * 1.5);
                zoom = Self::smooth_ema_1d(&zoom, 0.1);
            }
            SmoothingPreset::Default => {
                xy = Self::smooth_kalman(&xy);
                xy = Self::smooth_gaussian_2d(&xy, sigma);
                zoom = Self::smooth_gaussian_1d(&zoom, sigma);
                zoom = Self::smooth_ema_1d(&zoom, 0.2);
            }
        }

        // Recombine into trajectory points
        xy.iter()
            .zip(zoom.iter())
            .map(|((x, y), z)| TrajectoryPoint::new(*x, *y, *z))
            .collect()
    }

    /// Kalman filter for 2D trajectory (x, y with velocities)
    fn smooth_kalman(trajectory: &[(f64, f64)]) -> Vec<(f64, f64)> {
        if trajectory.is_empty() {
            return Vec::new();
        }

        let n = trajectory.len();
        let dt = 1.0; // per-frame time step

        // State transition matrix [x, y, vx, vy]
        #[rustfmt::skip]
        let f = DMatrix::from_row_slice(4, 4, &[
            1.0, 0.0, dt,  0.0,
            0.0, 1.0, 0.0, dt,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ]);

        // Observation matrix (we observe x, y)
        #[rustfmt::skip]
        let h = DMatrix::from_row_slice(2, 4, &[
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
        ]);

        // Process noise covariance
        let q = DMatrix::from_diagonal_element(4, 4, 1e-2);

        // Measurement noise covariance
        let r = DMatrix::from_diagonal_element(2, 2, 1.0);

        // Initial state
        let mut x_state = DVector::from_vec(vec![trajectory[0].0, trajectory[0].1, 0.0, 0.0]);
        let mut p = DMatrix::from_diagonal_element(4, 4, 100.0);

        let mut result = Vec::with_capacity(n);
        result.push(trajectory[0]);

        for i in 1..n {
            // Predict
            let x_pred = &f * &x_state;
            let p_pred = &f * &p * f.transpose() + &q;

            // Update
            let z = DVector::from_vec(vec![trajectory[i].0, trajectory[i].1]);
            let y_innov = z - &h * &x_pred;
            let s = &h * &p_pred * h.transpose() + &r;

            // Compute Kalman gain
            let s_inv = s.try_inverse().unwrap_or_else(|| DMatrix::identity(2, 2));
            let k = &p_pred * h.transpose() * s_inv;

            x_state = x_pred + &k * y_innov;
            p = (DMatrix::identity(4, 4) - &k * &h) * p_pred;

            result.push((x_state[0], x_state[1]));
        }

        result
    }

    /// Gaussian smoothing for 2D trajectory
    fn smooth_gaussian_2d(trajectory: &[(f64, f64)], sigma: f64) -> Vec<(f64, f64)> {
        if sigma < 0.5 || trajectory.len() < 3 {
            return trajectory.to_vec();
        }

        let x: Vec<f64> = trajectory.iter().map(|(x, _)| *x).collect();
        let y: Vec<f64> = trajectory.iter().map(|(_, y)| *y).collect();

        let x_smoothed = Self::smooth_gaussian_1d(&x, sigma);
        let y_smoothed = Self::smooth_gaussian_1d(&y, sigma);

        x_smoothed
            .iter()
            .zip(y_smoothed.iter())
            .map(|(x, y)| (*x, *y))
            .collect()
    }

    /// 1D Gaussian smoothing
    fn smooth_gaussian_1d(values: &[f64], sigma: f64) -> Vec<f64> {
        if sigma < 0.5 || values.len() < 3 {
            return values.to_vec();
        }

        let kernel_size = ((sigma * 6.0) as usize).max(3) | 1; // ensure odd
        let kernel_size = kernel_size.min(values.len() / 2 * 2 - 1);

        let kernel = Self::gaussian_kernel(kernel_size, sigma);
        Self::convolve_1d(values, &kernel)
    }

    /// Generate 1D Gaussian kernel
    fn gaussian_kernel(size: usize, sigma: f64) -> Vec<f64> {
        let mut kernel = Vec::with_capacity(size);
        let center = (size / 2) as f64;

        for i in 0..size {
            let x = i as f64 - center;
            let val = (-x * x / (2.0 * sigma * sigma)).exp();
            kernel.push(val);
        }

        // Normalize
        let sum: f64 = kernel.iter().sum();
        kernel.iter_mut().for_each(|v| *v /= sum);

        kernel
    }

    /// 1D convolution with edge padding
    fn convolve_1d(values: &[f64], kernel: &[f64]) -> Vec<f64> {
        let n = values.len();
        let k = kernel.len();
        let half_k = k / 2;

        let mut result = Vec::with_capacity(n);

        for i in 0..n {
            let mut sum = 0.0;
            let mut weight_sum = 0.0;

            for j in 0..k {
                let idx = i as i32 + j as i32 - half_k as i32;
                if idx >= 0 && idx < n as i32 {
                    sum += values[idx as usize] * kernel[j];
                    weight_sum += kernel[j];
                } else {
                    // Edge padding: use edge values
                    let edge_idx = if idx < 0 { 0 } else { n - 1 };
                    sum += values[edge_idx] * kernel[j];
                    weight_sum += kernel[j];
                }
            }

            result.push(sum / weight_sum);
        }

        result
    }

    /// Exponential moving average for 2D trajectory
    fn smooth_ema_2d(trajectory: &[(f64, f64)], alpha: f64) -> Vec<(f64, f64)> {
        if trajectory.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(trajectory.len());
        result.push(trajectory[0]);

        for i in 1..trajectory.len() {
            let prev = result[i - 1];
            let curr = trajectory[i];
            let smoothed = (
                alpha * curr.0 + (1.0 - alpha) * prev.0,
                alpha * curr.1 + (1.0 - alpha) * prev.1,
            );
            result.push(smoothed);
        }

        result
    }

    /// Exponential moving average for 1D values
    fn smooth_ema_1d(values: &[f64], alpha: f64) -> Vec<f64> {
        if values.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(values.len());
        result.push(values[0]);

        for i in 1..values.len() {
            let smoothed = alpha * values[i] + (1.0 - alpha) * result[i - 1];
            result.push(smoothed);
        }

        result
    }

    /// Apply ease-in-out interpolation for large jumps
    fn smooth_ease_in_out(
        trajectory: &[(f64, f64)],
        threshold_px: f64,
        transition_frames: usize,
    ) -> Vec<(f64, f64)> {
        let mut result = trajectory.to_vec();
        let n = result.len();
        let mut i = 0;

        while i < n {
            if i + 1 < n {
                let dx = result[i + 1].0 - result[i].0;
                let dy = result[i + 1].1 - result[i].1;
                let dist = (dx * dx + dy * dy).sqrt();

                if dist > threshold_px {
                    let start_pos = result[i];
                    let end_idx = (i + transition_frames).min(n - 1);
                    let end_pos = trajectory[end_idx];

                    // Cubic ease-in-out
                    for j in i..=end_idx {
                        let t = (j - i) as f64 / (end_idx - i).max(1) as f64;
                        let ease = 3.0 * t * t - 2.0 * t * t * t;

                        result[j] = (
                            start_pos.0 + ease * (end_pos.0 - start_pos.0),
                            start_pos.1 + ease * (end_pos.1 - start_pos.1),
                        );
                    }

                    i = end_idx + 1;
                    continue;
                }
            }
            i += 1;
        }

        result
    }
}

/// Interpolate window-level targets to per-frame trajectory
pub fn interpolate_to_frames(
    targets: &[WindowTarget],
    fps: f64,
    frame_count: usize,
    src_w: u32,
    src_h: u32,
    out_w: u32,
    out_h: u32,
    zoom_mode: ZoomMode,
    zoom_max: f64,
    padding: u32,
) -> Vec<TrajectoryPoint> {
    if targets.is_empty() {
        return vec![TrajectoryPoint::new(0.0, 0.0, 1.0); frame_count];
    }

    // Extract data from targets
    let ts: Vec<f64> = targets.iter().map(|t| t.timestamp).collect();
    let xs: Vec<f64> = targets.iter().map(|t| t.cx).collect();
    let ys: Vec<f64> = targets.iter().map(|t| t.cy).collect();
    let spreads: Vec<f64> = targets.iter().map(|t| t.spread).collect();
    let bbox_ws: Vec<f64> = targets.iter().map(|t| t.bbox_w).collect();
    let bbox_hs: Vec<f64> = targets.iter().map(|t| t.bbox_h).collect();

    let mut trajectory = Vec::with_capacity(frame_count);

    for frame_idx in 0..frame_count {
        let frame_time = frame_idx as f64 / fps;

        // Interpolate position
        let x = interp(&ts, &xs, frame_time);
        let y = interp(&ts, &ys, frame_time);

        // Compute zoom
        let zoom = match zoom_mode {
            ZoomMode::Auto => {
                let spread = interp(&ts, &spreads, frame_time);
                compute_zoom_from_spread(spread, src_w, src_h, zoom_max)
            }
            ZoomMode::Close => 1.5,
            ZoomMode::None => 1.0,
        };

        trajectory.push(TrajectoryPoint::new(x, y, zoom));
    }

    // Apply bbox-aware zoom limiting
    if zoom_mode != ZoomMode::None {
        apply_bbox_zoom_limits(
            &mut trajectory,
            &ts,
            &bbox_ws,
            &bbox_hs,
            fps,
            src_w,
            src_h,
            out_w,
            out_h,
            padding,
        );
    }

    trajectory
}

/// Linear interpolation
fn interp(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    if x <= xs[0] {
        return ys[0];
    }
    if x >= xs[xs.len() - 1] {
        return ys[ys.len() - 1];
    }

    // Find surrounding points
    for i in 0..xs.len() - 1 {
        if x >= xs[i] && x <= xs[i + 1] {
            let t = (x - xs[i]) / (xs[i + 1] - xs[i]);
            return ys[i] + t * (ys[i + 1] - ys[i]);
        }
    }

    ys[ys.len() - 1]
}

/// Compute zoom level from activity spread
fn compute_zoom_from_spread(spread: f64, src_w: u32, src_h: u32, zoom_max: f64) -> f64 {
    let frame_size = src_w.max(src_h) as f64;
    let norm = spread / frame_size; // ~0 to ~0.5

    // Invert: tight spread = high zoom, wide spread = zoom 1.0
    let raw_zoom = zoom_max - (zoom_max - 1.0) * (norm / 0.3).min(1.0);
    raw_zoom.max(1.0)
}

/// Apply bbox-aware zoom limits to prevent cropping out active regions
fn apply_bbox_zoom_limits(
    trajectory: &mut [TrajectoryPoint],
    ts: &[f64],
    bbox_ws: &[f64],
    bbox_hs: &[f64],
    fps: f64,
    src_w: u32,
    src_h: u32,
    out_w: u32,
    out_h: u32,
    padding: u32,
) {
    let out_aspect = out_w as f64 / out_h as f64;

    // Compute base crop size
    let (base_w, base_h) = if (src_w as f64 / src_h as f64) > out_aspect {
        let base_h = src_h.saturating_sub(2 * padding);
        let base_w = (base_h as f64 * out_aspect) as u32;
        (base_w as f64, base_h as f64)
    } else {
        let base_w = src_w.saturating_sub(2 * padding);
        let base_h = (base_w as f64 / out_aspect) as u32;
        (base_w as f64, base_h as f64)
    };

    for (frame_idx, point) in trajectory.iter_mut().enumerate() {
        let frame_time = frame_idx as f64 / fps;

        // Interpolate bbox dimensions
        let bbox_w = interp(ts, bbox_ws, frame_time);
        let bbox_h = interp(ts, bbox_hs, frame_time);

        // Add breathing room
        let margin = 1.2;
        let needed_w = bbox_w * margin;
        let needed_h = bbox_h * margin;

        // Only constrain if bbox is significant (> 10px)
        if needed_w > 10.0 && needed_h > 10.0 {
            let max_zoom_w = base_w / needed_w;
            let max_zoom_h = base_h / needed_h;
            let bbox_zoom_cap = max_zoom_w.min(max_zoom_h).max(1.0);

            if point.zoom > bbox_zoom_cap {
                point.zoom = bbox_zoom_cap;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpolation() {
        let xs = vec![0.0, 1.0, 2.0];
        let ys = vec![0.0, 10.0, 20.0];

        assert_eq!(interp(&xs, &ys, 0.5), 5.0);
        assert_eq!(interp(&xs, &ys, 1.5), 15.0);
        assert_eq!(interp(&xs, &ys, -1.0), 0.0); // clamp to start
        assert_eq!(interp(&xs, &ys, 3.0), 20.0); // clamp to end
    }

    #[test]
    fn test_gaussian_kernel() {
        let kernel = TrajectorySmoother::gaussian_kernel(5, 1.0);
        assert_eq!(kernel.len(), 5);

        // Sum should be 1.0 (normalized)
        let sum: f64 = kernel.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_ema() {
        let values = vec![0.0, 10.0, 20.0, 30.0];
        let smoothed = TrajectorySmoother::smooth_ema_1d(&values, 0.5);

        assert_eq!(smoothed[0], 0.0);
        assert!(smoothed[1] > 0.0 && smoothed[1] < 10.0);
    }
}
