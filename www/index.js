import init, { FitvidProcessor, get_platform_preset } from '../pkg/fitvid_wasm.js';

// Global state
let processor = null;
let videoElement = null;
let currentVideoFile = null;

// UI Elements
const uploadBox = document.getElementById('uploadBox');
const videoInput = document.getElementById('videoInput');
const optionsSection = document.getElementById('optionsSection');
const progressSection = document.getElementById('progressSection');
const previewSection = document.getElementById('previewSection');
const debugSection = document.getElementById('debugSection');
const processBtn = document.getElementById('processBtn');
const downloadBtn = document.getElementById('downloadBtn');
const resetBtn = document.getElementById('resetBtn');
const progressFill = document.getElementById('progressFill');
const progressText = document.getElementById('progressText');
const progressSubtext = document.getElementById('progressSubtext');
const outputVideo = document.getElementById('outputVideo');
const debugInfo = document.getElementById('debugInfo');
const smoothStrength = document.getElementById('smoothStrength');
const smoothStrengthValue = document.getElementById('smoothStrengthValue');

// Canvases
const sourceCanvas = document.getElementById('sourceCanvas');
const outputCanvas = document.getElementById('outputCanvas');
const sourceCtx = sourceCanvas.getContext('2d');
const outputCtx = outputCanvas.getContext('2d');

// Initialize WASM
async function initWasm() {
    try {
        await init();
        console.log('WASM initialized successfully');
    } catch (err) {
        console.error('Failed to initialize WASM:', err);
        alert('Failed to load WebAssembly module. Please refresh the page.');
    }
}

// Update progress UI
function updateProgress(percent, text, subtext = '') {
    progressFill.style.width = `${percent}%`;
    progressText.textContent = text;
    progressSubtext.textContent = subtext;
}

// Show/hide sections
function showSection(section) {
    const sections = [optionsSection, progressSection, previewSection];
    sections.forEach(s => s.style.display = 'none');
    section.style.display = 'block';
}

// Upload handlers
uploadBox.addEventListener('click', () => videoInput.click());

uploadBox.addEventListener('dragover', (e) => {
    e.preventDefault();
    uploadBox.classList.add('dragover');
});

uploadBox.addEventListener('dragleave', () => {
    uploadBox.classList.remove('dragover');
});

uploadBox.addEventListener('drop', (e) => {
    e.preventDefault();
    uploadBox.classList.remove('dragover');

    const files = e.dataTransfer.files;
    if (files.length > 0) {
        handleVideoUpload(files[0]);
    }
});

videoInput.addEventListener('change', (e) => {
    if (e.target.files.length > 0) {
        handleVideoUpload(e.target.files[0]);
    }
});

// Handle video upload
async function handleVideoUpload(file) {
    if (!file.type.startsWith('video/')) {
        alert('Please upload a video file');
        return;
    }

    currentVideoFile = file;
    console.log('Video uploaded:', file.name, file.size, 'bytes');

    // Create video element to get metadata
    videoElement = document.createElement('video');
    videoElement.src = URL.createObjectURL(file);

    videoElement.addEventListener('loadedmetadata', () => {
        const duration = videoElement.duration;
        const width = videoElement.videoWidth;
        const height = videoElement.videoHeight;

        console.log(`Video metadata: ${width}x${height}, ${duration.toFixed(1)}s`);

        // Optional: warn about long videos
        if (duration > 300) {
            const proceed = confirm(
                `This video is ${Math.floor(duration / 60)} minutes long.\n\n` +
                `Processing may take ${Math.floor(duration * 2 / 60)}-${Math.floor(duration * 3 / 60)} minutes.\n\n` +
                `Continue?`
            );
            if (!proceed) return;
        }

        debugInfo.textContent = `Source: ${width}x${height}, ${duration.toFixed(1)}s\nFile: ${file.name} (${(file.size / 1024 / 1024).toFixed(2)} MB)`;
        debugSection.style.display = 'block';

        showSection(optionsSection);
    });
}

// Smooth strength slider
smoothStrength.addEventListener('input', (e) => {
    smoothStrengthValue.textContent = e.target.value;
});

// Process button handler
processBtn.addEventListener('click', async () => {
    if (!videoElement) {
        alert('Please upload a video first');
        return;
    }

    showSection(progressSection);
    processBtn.disabled = true;

    try {
        await processVideo();
    } catch (err) {
        console.error('Processing error:', err);
        alert(`Processing failed: ${err.message || err}`);
        showSection(optionsSection);
    } finally {
        processBtn.disabled = false;
    }
});

// Main video processing function
async function processVideo() {
    const startTime = performance.now();

    // Get options
    const platformSelect = document.getElementById('platform');
    const smoothingSelect = document.getElementById('smoothing');
    const zoomSelect = document.getElementById('zoom');

    const preset = get_platform_preset(platformSelect.value);
    const options = {
        out_width: preset.width,
        out_height: preset.height,
        window_seconds: 2.0,
        threshold: 10,
        smoothing_preset: smoothingSelect.value,
        smooth_strength: parseFloat(smoothStrength.value),
        smooth_window: 2.0,
        zoom_mode: zoomSelect.value,
        zoom_max: 2.0,
        padding: 50,
        border_pct: 5.0,
    };

    console.log('Processing options:', options);

    // Create processor
    processor = new FitvidProcessor(options);

    const width = videoElement.videoWidth;
    const height = videoElement.videoHeight;
    const fps = 30; // Assume 30 fps (we'll extract frames at this rate)
    const duration = videoElement.duration;
    const frameCount = Math.floor(duration * fps);

    processor.set_video_metadata(width, height, fps);

    console.log(`Processing ${frameCount} frames at ${fps} fps`);

    // Phase 1: Decode and analyze frames
    updateProgress(5, 'Decoding video frames...', 'This may take a minute');

    const { frames: frameList, analysisHeight, analysisWidth } = await extractFrames(videoElement, fps, frameCount, (progress) => {
        updateProgress(5 + progress * 35, 'Extracting frames...', `${Math.floor(progress * 100)}%`);
    });

    console.log(`Extracted ${frameList.length} frames`);

    // Phase 2: Analyze activity
    updateProgress(40, 'Analyzing activity...', 'Finding regions of interest');

    const targetCount = processor.analyze();
    console.log(`Generated ${targetCount} activity targets`);

    // Scale targets back to full resolution if we downsampled
    if (analysisHeight < height) {
        processor.set_analysis_scale(analysisWidth, analysisHeight);
        console.log(`Scaled coordinates from ${analysisWidth}x${analysisHeight} to ${width}x${height}`);
    }

    const memoryMB = processor.memory_estimate_mb();
    console.log(`Memory usage: ${memoryMB.toFixed(2)} MB`);

    // Phase 3: Generate trajectory
    updateProgress(45, 'Generating smooth trajectory...', 'Computing camera path');

    const trajectoryCount = processor.generate_trajectory(frameCount);
    console.log(`Generated ${trajectoryCount} trajectory points`);

    // Phase 4: Encode output video
    updateProgress(50, 'Encoding output video...', 'Attempting to preserve audio');

    await encodeOutputVideo(videoElement, processor, frameCount, options, (progress) => {
        updateProgress(50 + progress * 50, 'Encoding video...', `Frame ${Math.floor(progress * frameCount)}/${frameCount}`);
    });

    const endTime = performance.now();
    const processingTime = ((endTime - startTime) / 1000).toFixed(1);

    console.log(`Processing complete in ${processingTime}s`);

    updateProgress(100, 'Complete!', `Processed in ${processingTime}s`);

    // Show preview
    console.log('Showing preview section...');
    setTimeout(() => {
        showSection(previewSection);
        console.log('Preview section visible:', previewSection.style.display);
    }, 500);
}

// Extract frames from video
async function extractFrames(video, fps, maxFrames, onProgress) {
    return new Promise((resolve, reject) => {
        const frames = [];
        const frameInterval = 1.0 / fps;
        let currentTime = 0;
        let frameIndex = 0;

        sourceCanvas.width = video.videoWidth;
        sourceCanvas.height = video.videoHeight;

        // Downsample to 720p for analysis (memory efficiency)
        const analysisHeight = Math.min(720, video.videoHeight);
        const analysisWidth = Math.floor(video.videoWidth * (analysisHeight / video.videoHeight));

        const captureFrame = () => {
            if (frameIndex >= maxFrames || currentTime >= video.duration) {
                resolve({ frames, analysisHeight, analysisWidth });
                return;
            }

            video.currentTime = currentTime;
        };

        video.addEventListener('seeked', function onSeeked() {
            try {
                // Draw current frame to canvas
                sourceCtx.drawImage(video, 0, 0);

                // Get image data
                const imageData = sourceCtx.getImageData(0, 0, video.videoWidth, video.videoHeight);

                // Add frame to processor (with downsampling)
                processor.add_frame(imageData, analysisHeight);

                frames.push(frameIndex);
                frameIndex++;
                currentTime += frameInterval;

                onProgress(frameIndex / maxFrames);

                if (frameIndex < maxFrames && currentTime < video.duration) {
                    setTimeout(captureFrame, 0); // Continue to next frame
                } else {
                    video.removeEventListener('seeked', onSeeked);
                    resolve({ frames, analysisHeight, analysisWidth });
                }
            } catch (err) {
                video.removeEventListener('seeked', onSeeked);
                reject(err);
            }
        }, false);

        // Start extraction
        captureFrame();
    });
}

// Encode output video - NEW APPROACH: Process first, then render with perfect timing
async function encodeOutputVideo(video, processor, frameCount, options, onProgress) {
    return new Promise(async (resolve, reject) => {
        try {
            // Get actual video FPS
            const videoFps = video.mozDecodedFrames ?
                (video.mozDecodedFrames / video.duration) :
                30;

            console.log(`Processing ${frameCount} frames at ${videoFps} fps`);

            // Setup canvases
            sourceCanvas.width = video.videoWidth;
            sourceCanvas.height = video.videoHeight;
            outputCanvas.width = options.out_width;
            outputCanvas.height = options.out_height;

            const borderPct = options.border_pct;
            const borderX = Math.floor(video.videoWidth * borderPct / 100);
            const borderY = Math.floor(video.videoHeight * borderPct / 100);

            // PHASE 1: Pre-process all frames and store as ImageBitmaps (efficient!)
            console.log('Phase 1: Pre-processing all frames...');
            const processedFrames = [];

            for (let i = 0; i < frameCount; i++) {
                const frameTime = i / videoFps;
                video.currentTime = frameTime;

                await new Promise(resolve => {
                    video.onseeked = resolve;
                });

                // Setup source canvas with border
                sourceCanvas.width = video.videoWidth + 2 * borderX;
                sourceCanvas.height = video.videoHeight + 2 * borderY;

                sourceCtx.fillStyle = 'black';
                sourceCtx.fillRect(0, 0, sourceCanvas.width, sourceCanvas.height);
                sourceCtx.drawImage(video, borderX, borderY);

                // Apply crop to output canvas
                processor.crop_frame(i, sourceCanvas, outputCanvas, borderPct);

                // Store as ImageBitmap for efficient rendering later
                const bitmap = await createImageBitmap(outputCanvas);
                processedFrames.push(bitmap);

                onProgress((i + 1) / frameCount * 0.7); // 0-70% for processing
            }

            console.log(`✅ Pre-processed ${processedFrames.length} frames`);

            // PHASE 2: Setup MediaRecorder with audio
            console.log('Phase 2: Setting up encoding with audio...');

            const canvasStream = outputCanvas.captureStream(0); // Manual control

            // Get audio track (requires video to be playing to capture audio)
            let audioTrack = null;
            let audioStream = null;

            // Mute video so user doesn't hear it
            video.muted = true;
            video.currentTime = 0;

            // Play video briefly to establish audio stream (user clicked button, so autoplay is OK)
            try {
                await video.play();
                console.log('Video playing (muted) to capture audio stream');

                // Now capture the stream with audio
                audioStream = video.captureStream();
                const audioTracks = audioStream.getAudioTracks();

                if (audioTracks.length > 0) {
                    audioTrack = audioTracks[0];
                    console.log('✅ Audio track captured:', audioTrack);
                } else {
                    console.warn('⚠️ No audio tracks in video');
                }

                // Pause for now, we'll play again during rendering
                video.pause();
                video.currentTime = 0;
            } catch (err) {
                console.warn('⚠️ Could not capture audio:', err);
                // Continue without audio
            }

            // Combine streams
            const combinedStream = new MediaStream();
            canvasStream.getVideoTracks().forEach(track => combinedStream.addTrack(track));
            if (audioTrack) {
                combinedStream.addTrack(audioTrack);
            }

            // Select codec
            let mimeType = 'video/webm';
            const codecs = ['video/webm;codecs=vp9', 'video/webm;codecs=vp8', 'video/webm'];
            for (const codec of codecs) {
                if (MediaRecorder.isTypeSupported(codec)) {
                    mimeType = codec;
                    console.log('Using codec:', codec);
                    break;
                }
            }

            const mediaRecorder = new MediaRecorder(combinedStream, {
                mimeType: mimeType,
                videoBitsPerSecond: 8000000,
            });

            const chunks = [];
            mediaRecorder.ondataavailable = (e) => {
                if (e.data.size > 0) {
                    chunks.push(e.data);
                }
            };

            mediaRecorder.onstop = () => {
                const blob = new Blob(chunks, { type: mimeType });
                const url = URL.createObjectURL(blob);

                console.log(`✅ Video encoded: ${(blob.size / 1024 / 1024).toFixed(2)} MB`);

                outputVideo.src = url;
                outputVideo.load();

                const extension = mimeType.includes('mp4') ? 'mp4' : 'webm';
                downloadBtn.onclick = () => {
                    const a = document.createElement('a');
                    a.href = url;
                    a.download = `fitvid_output.${extension}`;
                    a.click();
                };

                outputVideo.onloadedmetadata = () => {
                    console.log(`✅ Output ready: ${outputVideo.duration.toFixed(1)}s`);
                    resolve();
                };

                outputVideo.onerror = (err) => {
                    console.error('Error loading output:', err);
                    resolve();
                };
            };

            // PHASE 3: Render frames at perfect timing using requestAnimationFrame
            console.log('Phase 3: Rendering frames with perfect timing...');

            // Reset video to start (already muted from Phase 2)
            video.currentTime = 0;

            // Start MediaRecorder first
            mediaRecorder.start();

            // Small delay to ensure recording started
            await new Promise(resolve => setTimeout(resolve, 100));

            // NOW start video playback (muted, so user doesn't hear it)
            video.play().catch(err => console.warn('Video play failed:', err));

            const outputCtx = outputCanvas.getContext('2d');
            const startTime = performance.now();
            const frameDuration = 1000 / videoFps;
            let frameIndex = 0;

            const videoTrack = canvasStream.getVideoTracks()[0];

            function renderFrame(timestamp) {
                const elapsed = performance.now() - startTime;
                const targetFrame = Math.floor(elapsed / frameDuration);

                // Render all frames up to current time
                while (frameIndex <= targetFrame && frameIndex < processedFrames.length) {
                    outputCtx.drawImage(processedFrames[frameIndex], 0, 0);

                    // Request frame capture
                    if (videoTrack.requestFrame) {
                        videoTrack.requestFrame();
                    }

                    frameIndex++;
                    onProgress(0.7 + (frameIndex / frameCount) * 0.3); // 70-100%
                }

                if (frameIndex < processedFrames.length) {
                    requestAnimationFrame(renderFrame);
                } else {
                    // All frames rendered
                    console.log('All frames rendered, finalizing...');
                    video.pause();

                    setTimeout(() => {
                        mediaRecorder.stop();

                        // Cleanup bitmaps
                        processedFrames.forEach(bitmap => bitmap.close());
                    }, 500);
                }
            }

            requestAnimationFrame(renderFrame);

        } catch (err) {
            reject(err);
        }
    });
}

// Reset button handler
resetBtn.addEventListener('click', () => {
    if (processor) {
        processor.clear();
        processor = null;
    }

    if (videoElement) {
        URL.revokeObjectURL(videoElement.src);
        videoElement = null;
    }

    currentVideoFile = null;
    videoInput.value = '';

    optionsSection.style.display = 'none';
    progressSection.style.display = 'none';
    previewSection.style.display = 'none';
    debugSection.style.display = 'none';

    location.reload(); // Simple reset
});

// Initialize on page load
window.addEventListener('DOMContentLoaded', async () => {
    await initWasm();
});
