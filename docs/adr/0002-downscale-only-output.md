# Downscale-only output: never stretch pixels

Rendered output was always scaled to a fixed 1080×1920 canvas. From a 360p–720p source (most real uploads) that means a 4–5x upscale on the face crop — the visible "trash quality."

Decision: output size follows the native crop window, capped at 1080×1920. A 1080p source yields a ~607×1080 clip (exact 9:16, zero resampling); a 4K source downscales to 1080×1920; a 360p source yields ~202×360 — small but honest. The caption layer (ASS headers, font sizes) derives from actual output dimensions instead of fixed constants.
