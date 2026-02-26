# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "opencv-python>=4.8",
#     "numpy>=1.24",
# ]
# ///
"""fitvid — Smart crop screen recordings for social media.

Analyzes pixel activity in screen recordings, finds where the action is
happening over time, and dynamically pans/zooms to follow it — producing
smooth, professional-looking vertical video.

Usage:
    uv run fitvid.py INPUT_VIDEO -o OUTPUT_VIDEO --tiktok [options]
"""

from __future__ import annotations

import argparse
import math
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

import cv2
import numpy as np


# ---------------------------------------------------------------------------
# Platform presets
# ---------------------------------------------------------------------------

PLATFORMS: dict[str, tuple[int, int]] = {
    "tiktok": (1080, 1920),
    "reels": (1080, 1920),
    "ig-story": (1080, 1920),
    "ig-post": (1080, 1350),
    "ig-square": (1080, 1080),
    "fb-reel": (1080, 1920),
    "fb-story": (1080, 1920),
    "fb-feed": (1080, 1350),
    "yt-shorts": (1080, 1920),
    "twitter": (1080, 1920),
    "snapchat": (1080, 1920),
    "linkedin": (1080, 1080),
    "pinterest": (1080, 1920),
}


# ---------------------------------------------------------------------------
# Data types
# ---------------------------------------------------------------------------

@dataclass
class WindowTarget:
    """Activity target for a single analysis window."""
    timestamp: float  # center of window in seconds
    cx: float  # center x (pixels, in source frame coords)
    cy: float  # center y
    spread: float  # activity spread (std dev of active pixels)


# ---------------------------------------------------------------------------
# Phase 1: Activity analysis
# ---------------------------------------------------------------------------

def analyze_activity(
    video_path: str,
    window_seconds: float = 2.0,
    threshold: int = 10,
    *,
    preview_path: str | None = None,
) -> tuple[list[WindowTarget], int, int, float, int]:
    """Analyze pixel activity in overlapping time windows.

    Returns (targets, width, height, fps, frame_count).
    """
    cap = cv2.VideoCapture(video_path)
    if not cap.isOpened():
        sys.exit(f"Error: cannot open video '{video_path}'")

    fps = cap.get(cv2.CAP_PROP_FPS)
    width = int(cap.get(cv2.CAP_PROP_FRAME_WIDTH))
    height = int(cap.get(cv2.CAP_PROP_FRAME_HEIGHT))
    frame_count = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))

    if fps <= 0 or width <= 0 or height <= 0:
        sys.exit("Error: could not read video properties")

    window_frames = max(1, int(window_seconds * fps))
    step_frames = max(1, window_frames // 2)  # 50% overlap

    print(f"Video: {width}x{height} @ {fps:.1f}fps, {frame_count} frames "
          f"({frame_count / fps:.1f}s)")
    print(f"Analysis windows: {window_frames} frames ({window_seconds:.1f}s), "
          f"step {step_frames} frames")

    # Read all frames as grayscale
    print("Reading frames...")
    gray_frames: list[np.ndarray] = []
    while True:
        ret, frame = cap.read()
        if not ret:
            break
        gray_frames.append(cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY))
    cap.release()

    actual_count = len(gray_frames)
    print(f"Read {actual_count} frames")

    if actual_count < 2:
        sys.exit("Error: video too short (need at least 2 frames)")

    # Build targets per window
    targets: list[WindowTarget] = []
    threshold_val = int(threshold * 255 / 100)

    # Optional: accumulate a global heatmap for preview
    global_heatmap = np.zeros((height, width), dtype=np.float64) if preview_path else None

    window_start = 0
    while window_start < actual_count:
        window_end = min(window_start + window_frames, actual_count)
        if window_end - window_start < 2:
            break

        # Sum absolute diffs within window
        heatmap = np.zeros((height, width), dtype=np.float64)
        for i in range(window_start, window_end - 1):
            diff = cv2.absdiff(gray_frames[i], gray_frames[i + 1])
            heatmap += diff.astype(np.float64)

        # Threshold
        heatmap_norm = heatmap / max(heatmap.max(), 1e-6) * 255
        heatmap_u8 = heatmap_norm.astype(np.uint8)
        _, mask = cv2.threshold(heatmap_u8, threshold_val, 255, cv2.THRESH_BINARY)
        heatmap_masked = heatmap * (mask.astype(np.float64) / 255.0)

        if global_heatmap is not None:
            global_heatmap += heatmap_masked

        # Find centroid (weighted center of mass)
        total = heatmap_masked.sum()
        if total > 0:
            ys, xs = np.mgrid[0:height, 0:width]
            cx = float((xs * heatmap_masked).sum() / total)
            cy = float((ys * heatmap_masked).sum() / total)
            # Activity spread: weighted std
            spread_x = float(np.sqrt(((xs - cx) ** 2 * heatmap_masked).sum() / total))
            spread_y = float(np.sqrt(((ys - cy) ** 2 * heatmap_masked).sum() / total))
            spread = (spread_x + spread_y) / 2
        else:
            cx, cy = width / 2, height / 2
            spread = 0.0

        timestamp = ((window_start + window_end) / 2) / fps
        targets.append(WindowTarget(timestamp, cx, cy, spread))
        window_start += step_frames

    print(f"Generated {len(targets)} activity targets")

    # Save preview heatmap
    if preview_path and global_heatmap is not None:
        _save_preview(global_heatmap, preview_path, width, height)

    return targets, width, height, fps, actual_count


def _save_preview(
    heatmap: np.ndarray, path: str, width: int, height: int
) -> None:
    """Save heatmap visualization as an image."""
    norm = heatmap / max(heatmap.max(), 1e-6) * 255
    colored = cv2.applyColorMap(norm.astype(np.uint8), cv2.COLORMAP_JET)
    cv2.imwrite(path, colored)
    print(f"Preview heatmap saved to: {path}")


# ---------------------------------------------------------------------------
# Phase 2: Trajectory smoothing
# ---------------------------------------------------------------------------

def interpolate_to_frames(
    targets: list[WindowTarget], fps: float, frame_count: int
) -> np.ndarray:
    """Interpolate window-level targets to per-frame (x, y) trajectory."""
    if not targets:
        return np.zeros((frame_count, 2))

    # Timestamps and positions
    ts = np.array([t.timestamp for t in targets])
    xs = np.array([t.cx for t in targets])
    ys = np.array([t.cy for t in targets])

    frame_times = np.arange(frame_count) / fps
    traj_x = np.interp(frame_times, ts, xs)
    traj_y = np.interp(frame_times, ts, ys)

    return np.column_stack([traj_x, traj_y])


def smooth_kalman(traj: np.ndarray, process_noise: float = 1e-2) -> np.ndarray:
    """Apply 2D Kalman filter to trajectory. State = (x, y, vx, vy)."""
    n = len(traj)
    if n == 0:
        return traj

    dt = 1.0  # per-frame time step (normalized)

    # State transition
    F = np.array([
        [1, 0, dt, 0],
        [0, 1, 0, dt],
        [0, 0, 1, 0],
        [0, 0, 0, 1],
    ], dtype=np.float64)

    # Observation matrix (we observe x, y)
    H = np.array([
        [1, 0, 0, 0],
        [0, 1, 0, 0],
    ], dtype=np.float64)

    # Process noise
    Q = np.eye(4) * process_noise
    # Measurement noise
    R = np.eye(2) * 1.0

    # Initial state
    x_state = np.array([traj[0, 0], traj[0, 1], 0, 0], dtype=np.float64)
    P = np.eye(4) * 100.0

    result = np.zeros_like(traj)
    result[0] = traj[0]

    for i in range(1, n):
        # Predict
        x_pred = F @ x_state
        P_pred = F @ P @ F.T + Q

        # Update
        z = traj[i]
        y_innov = z - H @ x_pred
        S = H @ P_pred @ H.T + R
        K = P_pred @ H.T @ np.linalg.inv(S)

        x_state = x_pred + K @ y_innov
        P = (np.eye(4) - K @ H) @ P_pred

        result[i] = x_state[:2]

    return result


def smooth_gaussian(traj: np.ndarray, sigma: float) -> np.ndarray:
    """Apply 1D Gaussian smoothing to each axis."""
    if sigma < 0.5 or len(traj) < 3:
        return traj

    kernel_size = int(sigma * 6) | 1  # ensure odd
    kernel_size = max(3, min(kernel_size, len(traj) // 2 * 2 - 1))

    smoothed = np.copy(traj)
    for axis in range(2):
        col = traj[:, axis].astype(np.float64)
        # Pad with edge values
        padded = np.pad(col, kernel_size, mode="edge")
        blurred = cv2.GaussianBlur(
            padded.reshape(1, -1), (kernel_size, 1), sigma
        ).flatten()
        smoothed[:, axis] = blurred[kernel_size:-kernel_size]

    return smoothed


def smooth_ema(traj: np.ndarray, alpha: float = 0.3) -> np.ndarray:
    """Exponential moving average."""
    result = np.copy(traj)
    for i in range(1, len(result)):
        result[i] = alpha * traj[i] + (1 - alpha) * result[i - 1]
    return result


def smooth_ease_in_out(
    traj: np.ndarray, threshold_px: float = 50.0, transition_frames: int = 30
) -> np.ndarray:
    """Apply ease-in-out interpolation when target shifts significantly."""
    result = np.copy(traj)
    n = len(result)
    i = 0
    while i < n:
        # Look ahead for a big jump
        if i + 1 < n:
            dist = np.linalg.norm(result[i + 1] - result[i])
            if dist > threshold_px:
                # Find end of jump region
                start_pos = result[i].copy()
                end_idx = min(i + transition_frames, n - 1)
                end_pos = traj[end_idx].copy()
                # Cubic ease-in-out
                for j in range(i, end_idx + 1):
                    t = (j - i) / max(1, end_idx - i)
                    # Cubic bezier ease-in-out: 3t^2 - 2t^3
                    ease = 3 * t * t - 2 * t * t * t
                    result[j] = start_pos + ease * (end_pos - start_pos)
                i = end_idx + 1
                continue
        i += 1
    return result


def apply_smoothing(
    traj: np.ndarray,
    preset: str,
    fps: float,
    smooth_window: float,
    smooth_strength: float,
) -> np.ndarray:
    """Apply the selected smoothing pipeline."""
    sigma = smooth_window * fps * smooth_strength

    if preset == "snappy":
        alpha = 0.15 + 0.35 * smooth_strength  # range ~0.15 to 0.5
        return smooth_ema(traj, alpha=alpha)
    elif preset == "cinematic":
        traj = smooth_kalman(traj, process_noise=5e-3)
        traj = smooth_gaussian(traj, sigma=sigma)
        traj = smooth_ease_in_out(
            traj,
            threshold_px=50,
            transition_frames=int(fps * 0.8),
        )
        return traj
    else:  # default
        traj = smooth_kalman(traj, process_noise=1e-2)
        traj = smooth_gaussian(traj, sigma=sigma)
        return traj


# ---------------------------------------------------------------------------
# Phase 3: Encoding
# ---------------------------------------------------------------------------

def compute_crop_box(
    cx: float,
    cy: float,
    src_w: int,
    src_h: int,
    out_w: int,
    out_h: int,
    padding: int,
) -> tuple[int, int, int, int]:
    """Compute crop box (x, y, w, h) centered on (cx, cy).

    The crop region has the same aspect ratio as the output, is as large as
    possible within the source frame, and is clamped to frame bounds.
    """
    out_aspect = out_w / out_h

    # Determine crop size: largest rectangle with output aspect that fits source
    if src_w / src_h > out_aspect:
        # Source is wider than needed — height-limited
        crop_h = src_h - 2 * padding
        crop_w = int(crop_h * out_aspect)
    else:
        # Source is taller than needed — width-limited
        crop_w = src_w - 2 * padding
        crop_h = int(crop_w / out_aspect)

    crop_w = max(1, crop_w)
    crop_h = max(1, crop_h)

    # Center crop on the target point, clamp to frame
    x = int(cx - crop_w / 2)
    y = int(cy - crop_h / 2)
    x = max(0, min(x, src_w - crop_w))
    y = max(0, min(y, src_h - crop_h))

    return x, y, crop_w, crop_h


def encode_video(
    input_path: str,
    output_path: str,
    traj: np.ndarray,
    src_w: int,
    src_h: int,
    fps: float,
    frame_count: int,
    out_w: int,
    out_h: int,
    padding: int,
) -> None:
    """Read frames, crop, resize, write output, then mux audio."""
    tmp_dir = tempfile.mkdtemp(prefix="fitvid_")
    tmp_video = str(Path(tmp_dir) / "video_only.mp4")

    fourcc = cv2.VideoWriter.fourcc(*"mp4v")
    writer = cv2.VideoWriter(tmp_video, fourcc, fps, (out_w, out_h))
    if not writer.isOpened():
        sys.exit("Error: could not create video writer")

    cap = cv2.VideoCapture(input_path)
    if not cap.isOpened():
        sys.exit(f"Error: cannot reopen video '{input_path}'")

    n_frames = min(len(traj), frame_count)
    print(f"Encoding {n_frames} frames to {out_w}x{out_h}...")

    for i in range(n_frames):
        ret, frame = cap.read()
        if not ret:
            break

        cx, cy = traj[i]
        x, y, cw, ch = compute_crop_box(cx, cy, src_w, src_h, out_w, out_h, padding)
        cropped = frame[y : y + ch, x : x + cw]
        resized = cv2.resize(cropped, (out_w, out_h), interpolation=cv2.INTER_LANCZOS4)
        writer.write(resized)

        if (i + 1) % 500 == 0 or i == n_frames - 1:
            pct = (i + 1) / n_frames * 100
            print(f"  {i + 1}/{n_frames} ({pct:.0f}%)")

    cap.release()
    writer.release()
    print("Video written, muxing audio...")

    # Mux audio from original using FFmpeg
    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        # No FFmpeg — just copy the video without audio
        print("Warning: ffmpeg not found, output will have no audio")
        shutil.move(tmp_video, output_path)
    else:
        cmd = [
            ffmpeg, "-y",
            "-i", tmp_video,
            "-i", input_path,
            "-c:v", "libx264",
            "-preset", "medium",
            "-crf", "18",
            "-map", "0:v:0",
            "-map", "1:a:0?",
            "-c:a", "aac",
            "-b:a", "192k",
            "-movflags", "+faststart",
            "-shortest",
            output_path,
        ]
        result = subprocess.run(cmd, capture_output=True, text=True)
        if result.returncode != 0:
            print(f"FFmpeg stderr:\n{result.stderr}", file=sys.stderr)
            sys.exit(f"Error: FFmpeg exited with code {result.returncode}")

    # Cleanup
    try:
        shutil.rmtree(tmp_dir)
    except OSError:
        pass

    print(f"Output saved to: {output_path}")


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="fitvid",
        description="Smart crop screen recordings for social media",
    )
    parser.add_argument("input", help="Input video file")
    parser.add_argument("-o", "--output", required=True, help="Output video file")

    # Platform presets (mutually exclusive group)
    platform = parser.add_mutually_exclusive_group(required=True)
    for name, (w, h) in PLATFORMS.items():
        platform.add_argument(
            f"--{name}",
            dest="platform",
            action="store_const",
            const=(w, h),
            help=f"{w}x{h}",
        )
    platform.add_argument(
        "--custom",
        dest="platform",
        type=_parse_resolution,
        metavar="WxH",
        help="Custom resolution, e.g. 1080x1920",
    )

    # Smoothing preset
    parser.add_argument(
        "--smooth",
        choices=["default", "snappy", "cinematic"],
        default="default",
        help="Smoothing preset (default: default)",
    )

    # Manual tuning
    parser.add_argument(
        "--smooth-window",
        type=float,
        default=2.0,
        help="Analysis window size in seconds (default: 2.0)",
    )
    parser.add_argument(
        "--smooth-strength",
        type=float,
        default=0.5,
        help="Smoothing strength 0.0-1.0 (default: 0.5)",
    )

    # Other flags
    parser.add_argument(
        "--preview",
        action="store_true",
        help="Save activity heatmap image alongside output",
    )
    parser.add_argument(
        "--padding",
        type=int,
        default=50,
        help="Extra pixels around detected region (default: 50)",
    )
    parser.add_argument(
        "--threshold",
        type=int,
        default=10,
        help="Activity threshold 0-100 to filter noise (default: 10)",
    )

    return parser


def _parse_resolution(value: str) -> tuple[int, int]:
    """Parse 'WxH' string into (width, height) tuple."""
    try:
        w, h = value.lower().split("x")
        return (int(w), int(h))
    except (ValueError, AttributeError):
        raise argparse.ArgumentTypeError(
            f"Invalid resolution '{value}', expected WxH (e.g. 1080x1920)"
        )


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()

    input_path = args.input
    output_path = args.output
    out_w, out_h = args.platform

    if not Path(input_path).is_file():
        sys.exit(f"Error: input file not found: {input_path}")

    # Preview path
    preview_path: str | None = None
    if args.preview:
        stem = Path(output_path).stem
        preview_path = str(Path(output_path).with_name(f"{stem}_heatmap.png"))

    print(f"fitvid — cropping to {out_w}x{out_h}")
    print(f"Smooth: {args.smooth} (window={args.smooth_window}s, "
          f"strength={args.smooth_strength})")

    # Phase 1: Analyze
    print("\n=== Phase 1: Activity Analysis ===")
    targets, src_w, src_h, fps, frame_count = analyze_activity(
        input_path,
        window_seconds=args.smooth_window,
        threshold=args.threshold,
        preview_path=preview_path,
    )

    # Phase 2: Smooth
    print("\n=== Phase 2: Trajectory Smoothing ===")
    traj = interpolate_to_frames(targets, fps, frame_count)
    traj = apply_smoothing(
        traj, args.smooth, fps, args.smooth_window, args.smooth_strength
    )
    print(f"Smoothed trajectory: {len(traj)} frames")

    # Phase 3: Encode
    print("\n=== Phase 3: Encoding ===")
    encode_video(
        input_path,
        output_path,
        traj,
        src_w,
        src_h,
        fps,
        frame_count,
        out_w,
        out_h,
        args.padding,
    )

    print("\nDone!")


if __name__ == "__main__":
    main()
