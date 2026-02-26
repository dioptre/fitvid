use crate::types::{MemoryEfficientFrame, WindowTarget};
use crate::utils::log_str;
use std::collections::VecDeque;

/// Activity analyzer that processes frames in sliding windows
pub struct ActivityAnalyzer {
    window_frames: usize,
    threshold: u8,
}

impl ActivityAnalyzer {
    pub fn new(window_frames: usize, threshold: u8) -> Self {
        ActivityAnalyzer {
            window_frames,
            threshold,
        }
    }

    /// Analyze activity in a sliding window of frames
    /// Returns activity targets for the window
    pub fn analyze_window(
        &self,
        frames: &VecDeque<MemoryEfficientFrame>,
        window_start_idx: usize,
        fps: f64,
    ) -> Option<WindowTarget> {
        if frames.len() < 2 {
            return None;
        }

        let width = frames[0].width;
        let height = frames[0].height;
        let size = (width * height) as usize;

        // Compute heatmap from frame differencing
        let mut heatmap = vec![0.0f64; size];

        for i in 0..frames.len() - 1 {
            let frame1 = &frames[i];
            let frame2 = &frames[i + 1];

            for idx in 0..size {
                let diff = (frame1.grayscale_data[idx] as i32
                          - frame2.grayscale_data[idx] as i32).abs();
                heatmap[idx] += diff as f64;
            }
        }

        // Normalize and threshold
        let max_val = heatmap.iter().copied().fold(0.0f64, f64::max);
        if max_val < 1e-6 {
            // No activity detected
            return self.default_target(width, height, window_start_idx, fps);
        }

        // Normalize to 0-255 range
        let threshold_val = (self.threshold as f64 * 255.0 / 100.0) as u8;
        let mut heatmap_masked = vec![0.0f64; size];

        for i in 0..size {
            let normalized = (heatmap[i] / max_val * 255.0) as u8;
            if normalized >= threshold_val {
                heatmap_masked[i] = heatmap[i];
            }
        }

        // Calculate centroid and spread
        let total: f64 = heatmap_masked.iter().sum();
        if total < 1.0 {
            return self.default_target(width, height, window_start_idx, fps);
        }

        let mut cx = 0.0;
        let mut cy = 0.0;

        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) as usize;
                let weight = heatmap_masked[idx];
                cx += x as f64 * weight;
                cy += y as f64 * weight;
            }
        }

        cx /= total;
        cy /= total;

        // Calculate spread (weighted standard deviation)
        let mut spread_x = 0.0;
        let mut spread_y = 0.0;

        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) as usize;
                let weight = heatmap_masked[idx];
                spread_x += (x as f64 - cx).powi(2) * weight;
                spread_y += (y as f64 - cy).powi(2) * weight;
            }
        }

        spread_x = (spread_x / total).sqrt();
        spread_y = (spread_y / total).sqrt();
        let spread = (spread_x + spread_y) / 2.0;

        // Calculate bounding box
        let (bbox_w, bbox_h) = self.calculate_bounding_box(&heatmap_masked, width, height, threshold_val);

        // Calculate timestamp (center of window)
        let window_center_frame = window_start_idx + frames.len() / 2;
        let timestamp = window_center_frame as f64 / fps;

        Some(WindowTarget {
            timestamp,
            cx,
            cy,
            spread,
            bbox_w,
            bbox_h,
        })
    }

    /// Calculate bounding box of active pixels
    fn calculate_bounding_box(
        &self,
        heatmap: &[f64],
        width: u32,
        height: u32,
        threshold: u8,
    ) -> (f64, f64) {
        let mut min_x = width;
        let mut max_x = 0u32;
        let mut min_y = height;
        let mut max_y = 0u32;

        let threshold_f = threshold as f64;
        let mut found_any = false;

        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) as usize;
                if heatmap[idx] > threshold_f {
                    found_any = true;
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }

        if found_any && max_x >= min_x && max_y >= min_y {
            let bbox_w = (max_x - min_x) as f64;
            let bbox_h = (max_y - min_y) as f64;
            (bbox_w, bbox_h)
        } else {
            (0.0, 0.0)
        }
    }

    /// Return default target (center of frame) when no activity is detected
    fn default_target(
        &self,
        width: u32,
        height: u32,
        window_start_idx: usize,
        fps: f64,
    ) -> Option<WindowTarget> {
        let timestamp = (window_start_idx + self.window_frames / 2) as f64 / fps;
        Some(WindowTarget {
            timestamp,
            cx: width as f64 / 2.0,
            cy: height as f64 / 2.0,
            spread: 0.0,
            bbox_w: 0.0,
            bbox_h: 0.0,
        })
    }
}

/// Streaming activity processor using sliding windows
pub struct SlidingWindowProcessor {
    window_size: usize,
    overlap: usize,
    analyzer: ActivityAnalyzer,
}

impl SlidingWindowProcessor {
    pub fn new(window_seconds: f64, fps: f64, threshold: u8) -> Self {
        // Use smaller window for short videos (min 10 frames, max 600)
        let ideal_window = (window_seconds * fps) as usize;
        let window_size = ideal_window.max(10).min(600);
        let overlap = window_size / 2; // 50% overlap

        log_str(&format!(
            "Sliding window: {} frames ({:.1}s), overlap: {} frames",
            window_size,
            window_seconds,
            overlap
        ));

        SlidingWindowProcessor {
            window_size,
            overlap,
            analyzer: ActivityAnalyzer::new(window_size, threshold),
        }
    }

    /// Process a batch of frames and extract activity targets
    pub fn process_frames(
        &self,
        frames: &[MemoryEfficientFrame],
        fps: f64,
    ) -> Vec<WindowTarget> {
        let mut targets = Vec::new();

        // For very short videos, use smaller window size
        let effective_window_size = self.window_size.min(frames.len());
        let step = (self.window_size - self.overlap).max(10); // Minimum step of 10 frames

        let mut window_start = 0;

        while window_start < frames.len() {
            let window_end = (window_start + effective_window_size).min(frames.len());

            if window_end - window_start < 2 {
                break;
            }

            // Create window slice
            let window_frames: VecDeque<MemoryEfficientFrame> =
                frames[window_start..window_end].iter().cloned().collect();

            // Analyze current window
            if let Some(target) = self.analyzer.analyze_window(
                &window_frames,
                window_start,
                fps
            ) {
                targets.push(target);
            }

            // Move to next window
            window_start += step;

            // Ensure we get at least 3 windows for short videos
            if window_start >= frames.len() && targets.len() < 3 && window_start > step {
                // Go back and create one more window from the end
                window_start = frames.len().saturating_sub(effective_window_size);
                if window_start > 0 && targets.last().map_or(true, |t| {
                    let last_time = t.timestamp;
                    let new_time = (window_start + effective_window_size / 2) as f64 / fps;
                    (new_time - last_time).abs() > 0.5 // At least 0.5s different
                }) {
                    let window_frames: VecDeque<MemoryEfficientFrame> =
                        frames[window_start..frames.len()].iter().cloned().collect();
                    if let Some(target) = self.analyzer.analyze_window(
                        &window_frames,
                        window_start,
                        fps
                    ) {
                        targets.push(target);
                    }
                }
                break;
            }
        }

        targets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_activity_analyzer_no_activity() {
        let analyzer = ActivityAnalyzer::new(10, 10);

        // Create identical frames (no activity)
        let mut frames = VecDeque::new();
        for _ in 0..10 {
            let frame = MemoryEfficientFrame {
                grayscale_data: vec![128u8; 100],
                width: 10,
                height: 10,
            };
            frames.push_back(frame);
        }

        let target = analyzer.analyze_window(&frames, 0, 30.0);
        assert!(target.is_some());

        let target = target.unwrap();
        // Should default to center
        assert_eq!(target.cx, 5.0);
        assert_eq!(target.cy, 5.0);
    }

    #[test]
    fn test_activity_analyzer_with_activity() {
        let analyzer = ActivityAnalyzer::new(3, 5);

        let mut frames = VecDeque::new();

        // First frame - dark
        frames.push_back(MemoryEfficientFrame {
            grayscale_data: vec![0u8; 100],
            width: 10,
            height: 10,
        });

        // Second frame - bright spot in corner
        let mut data = vec![0u8; 100];
        data[0] = 255;
        data[1] = 255;
        frames.push_back(MemoryEfficientFrame {
            grayscale_data: data,
            width: 10,
            height: 10,
        });

        let target = analyzer.analyze_window(&frames, 0, 30.0);
        assert!(target.is_some());
    }
}
