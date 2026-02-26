# 🎬 FitVid - Smart Video Cropping

Smart crop screen recordings for social media. Analyzes pixel activity, finds where the action is happening over time, and dynamically pans and zooms to follow it — producing smooth, professional-looking vertical video.

## 🌐 WebAssembly Version (Browser-Based)

**NEW**: Run FitVid directly in your browser with zero installation!

The Rust + WebAssembly port brings FitVid to the web with:
- ✅ Zero installation - runs entirely in your browser
- ✅ Privacy-preserving - all processing happens locally
- ✅ Memory-efficient - processes videos of any length
- ✅ Cross-platform - works on any device with a modern browser

**[📖 See QUICKSTART.md for complete setup & architecture details](QUICKSTART.md)**

---

## 🐍 Python Version (CLI Tool)

No paid tools. Just OpenCV + FFmpeg. 

## TL;DR
```
uv run --python 3.12 fitvid.py robinhood.mp4 -o robinhood_tiktok.mp4 --tiktok
```

## Requirements

- [uv](https://docs.astral.sh/uv/) (manages Python + dependencies automatically)
- FFmpeg (for audio muxing and final encoding)

## Usage

```bash
uv run fitvid.py INPUT_VIDEO -o OUTPUT_VIDEO --PLATFORM [options]
```

### Quick examples

```bash
# TikTok / Reels / Shorts (9:16)
uv run fitvid.py recording.mp4 -o tiktok.mp4 --tiktok

# Instagram square feed
uv run fitvid.py recording.mp4 -o square.mp4 --ig-square

# Cinematic smooth with close zoom
uv run fitvid.py recording.mp4 -o cinematic.mp4 --tiktok --smooth=cinematic --zoom=close

# Custom resolution
uv run fitvid.py recording.mp4 -o custom.mp4 --custom 720x1280
```

## Platform presets

| Flag | Resolution | Ratio | Use case |
|---|---|---|---|
| `--tiktok` | 1080x1920 | 9:16 | TikTok |
| `--reels` | 1080x1920 | 9:16 | Instagram Reels |
| `--ig-story` | 1080x1920 | 9:16 | Instagram Stories |
| `--ig-post` | 1080x1350 | 4:5 | IG portrait feed |
| `--ig-square` | 1080x1080 | 1:1 | IG square feed |
| `--fb-reel` | 1080x1920 | 9:16 | Facebook Reels |
| `--fb-story` | 1080x1920 | 9:16 | Facebook Stories |
| `--fb-feed` | 1080x1350 | 4:5 | Facebook feed |
| `--yt-shorts` | 1080x1920 | 9:16 | YouTube Shorts |
| `--twitter` | 1080x1920 | 9:16 | X/Twitter |
| `--snapchat` | 1080x1920 | 9:16 | Snapchat |
| `--linkedin` | 1080x1080 | 1:1 | LinkedIn |
| `--pinterest` | 1080x1920 | 9:16 | Pinterest |
| `--custom WxH` | any | derived | Custom resolution |

## Smoothing presets

| `--smooth=` | Pipeline | Character |
|---|---|---|
| `default` | Kalman filter → Gaussian | Smooth all-rounder |
| `snappy` | EMA (low alpha) | Quick, responsive tracking |
| `cinematic` | Kalman → Gaussian → ease-in-out | Slow dramatic pans |

## Zoom modes

| `--zoom=` | Behavior |
|---|---|
| `auto` | Zooms in on concentrated activity, zooms out when spread wide (default) |
| `none` | Fixed crop, no zoom (pan only) |
| `close` | Constant 1.5x zoom for a tighter crop |

## All options

| Option | Default | Description |
|---|---|---|
| `--smooth` | `default` | Smoothing preset |
| `--smooth-window` | `2.0` | Analysis window size in seconds |
| `--smooth-strength` | `0.5` | Smoothing strength (0.0–1.0) |
| `--zoom` | `auto` | Zoom mode |
| `--zoom-max` | `2.0` | Max zoom factor for auto mode |
| `--border-size` | `10` | Black border as % of frame size (0–200). Gives the crop room to pull back beyond source edges. |
| `--padding` | `50` | Extra pixels around detected region |
| `--threshold` | `10` | Activity threshold (0–100) to filter noise |
| `--preview` | off | Save activity heatmap image |

## How it works

1. **Analyze** — Divides video into overlapping time windows, computes frame-to-frame pixel diffs, finds the weighted center of activity, its spread, and bounding box in each window
2. **Smooth** — Interpolates targets to per-frame positions, applies the selected smoothing pipeline (Kalman, Gaussian, EMA, ease-in-out) to both the pan trajectory and zoom level. Zoom is capped per-frame so the active bounding box always fits in the crop.
3. **Encode** — Pads each frame with a black border (default 5%), crops at the smoothed position and zoom level, resizes to target resolution, muxes audio from the original via FFmpeg
