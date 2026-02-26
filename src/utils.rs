use wasm_bindgen::prelude::*;
use web_sys::console;

/// Console logging macro for debugging
#[macro_export]
macro_rules! log {
    ($($t:tt)*) => {
        web_sys::console::log_1(&format!($($t)*).into());
    };
}

/// Log to console
pub fn log_str(s: &str) {
    console::log_1(&s.into());
}

/// Log an error to console
pub fn log_error(s: &str) {
    console::error_1(&s.into());
}

/// Performance timing helper
pub struct Timer {
    start: f64,
    label: String,
}

impl Timer {
    pub fn new(label: &str) -> Self {
        let start = web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now())
            .unwrap_or(0.0);

        log_str(&format!("[Timer] {} started", label));

        Timer {
            start,
            label: label.to_string(),
        }
    }

    pub fn elapsed(&self) -> f64 {
        let now = web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now())
            .unwrap_or(0.0);
        now - self.start
    }

    pub fn log_elapsed(&self) {
        let elapsed = self.elapsed();
        log_str(&format!("[Timer] {} took {:.2}ms", self.label, elapsed));
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        self.log_elapsed();
    }
}

/// Compute crop box centered on (cx, cy) with the correct aspect ratio
/// Returns (x, y, width, height)
pub fn compute_crop_box(
    cx: f64,
    cy: f64,
    src_w: u32,
    src_h: u32,
    out_w: u32,
    out_h: u32,
    padding: u32,
    zoom: f64,
) -> (u32, u32, u32, u32) {
    let out_aspect = out_w as f64 / out_h as f64;
    let zoom = zoom.max(0.5); // floor at 0.5 to prevent extreme zoom-out

    // Determine base crop size
    let (base_w, base_h) = if (src_w as f64 / src_h as f64) > out_aspect {
        let base_h = src_h.saturating_sub(2 * padding);
        let base_w = (base_h as f64 * out_aspect) as u32;
        (base_w, base_h)
    } else {
        let base_w = src_w.saturating_sub(2 * padding);
        let base_h = (base_w as f64 / out_aspect) as u32;
        (base_w, base_h)
    };

    // Apply zoom
    let crop_w = ((base_w as f64 / zoom) as u32).max(1).min(src_w);
    let crop_h = ((base_h as f64 / zoom) as u32).max(1).min(src_h);

    // Center crop on target point
    let x = ((cx - crop_w as f64 / 2.0) as i32)
        .max(0)
        .min((src_w - crop_w) as i32) as u32;
    let y = ((cy - crop_h as f64 / 2.0) as i32)
        .max(0)
        .min((src_h - crop_h) as i32) as u32;

    (x, y, crop_w, crop_h)
}

/// Convert RGBA ImageData to grayscale
/// Uses standard luminance formula: Y = 0.299*R + 0.587*G + 0.114*B
pub fn rgba_to_grayscale(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let pixel_count = (width * height) as usize;
    let mut gray = Vec::with_capacity(pixel_count);

    for i in 0..pixel_count {
        let idx = i * 4;
        let r = rgba[idx] as f64;
        let g = rgba[idx + 1] as f64;
        let b = rgba[idx + 2] as f64;

        // Standard luminance formula
        let gray_val = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
        gray.push(gray_val);
    }

    gray
}

/// Downsample grayscale image using simple averaging
/// Useful for reducing memory usage during analysis
pub fn downsample_grayscale(
    data: &[u8],
    src_w: u32,
    src_h: u32,
    target_h: u32,
) -> (Vec<u8>, u32, u32) {
    // Calculate target width maintaining aspect ratio
    let aspect = src_w as f64 / src_h as f64;
    let target_w = (target_h as f64 * aspect) as u32;

    let scale_x = src_w as f64 / target_w as f64;
    let scale_y = src_h as f64 / target_h as f64;

    let mut downsampled = Vec::with_capacity((target_w * target_h) as usize);

    for ty in 0..target_h {
        for tx in 0..target_w {
            // Map target pixel to source region
            let sx_start = (tx as f64 * scale_x) as u32;
            let sy_start = (ty as f64 * scale_y) as u32;
            let sx_end = ((tx + 1) as f64 * scale_x).min(src_w as f64) as u32;
            let sy_end = ((ty + 1) as f64 * scale_y).min(src_h as f64) as u32;

            // Average pixels in the region
            let mut sum = 0u32;
            let mut count = 0u32;

            for sy in sy_start..sy_end {
                for sx in sx_start..sx_end {
                    let idx = (sy * src_w + sx) as usize;
                    sum += data[idx] as u32;
                    count += 1;
                }
            }

            let avg = if count > 0 { sum / count } else { 0 };
            downsampled.push(avg as u8);
        }
    }

    (downsampled, target_w, target_h)
}

/// Clamp value between min and max
#[inline]
pub fn clamp<T: PartialOrd>(val: T, min: T, max: T) -> T {
    if val < min {
        min
    } else if val > max {
        max
    } else {
        val
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgba_to_grayscale() {
        let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255]; // Red, Green pixels
        let gray = rgba_to_grayscale(&rgba, 2, 1);

        assert_eq!(gray.len(), 2);
        // Red should be ~76 (0.299 * 255)
        assert!((gray[0] as i32 - 76).abs() < 5);
        // Green should be ~150 (0.587 * 255)
        assert!((gray[1] as i32 - 150).abs() < 5);
    }

    #[test]
    fn test_compute_crop_box() {
        let (x, y, w, h) = compute_crop_box(
            960.0, 540.0,  // center of 1920x1080
            1920, 1080,
            1080, 1920,
            50,
            1.0,
        );

        // Should return a valid crop box
        assert!(x + w <= 1920);
        assert!(y + h <= 1080);
        assert!(w > 0);
        assert!(h > 0);
    }

    #[test]
    fn test_clamp() {
        assert_eq!(clamp(5, 0, 10), 5);
        assert_eq!(clamp(-5, 0, 10), 0);
        assert_eq!(clamp(15, 0, 10), 10);
    }
}
