# libx264 software encoding over VideoToolbox

Commit `c7d258b` switched macOS renders to `h264_videotoolbox -q:v 60` for 3–5x faster encodes. Rendered clips visibly degraded — the VideoToolbox quality slider doesn't track libx264 CRF semantics, and every clip pays the cost twice (base render + caption burn).

Decision: encode final output with libx264 at high quality (crf ~17, preset fast) on all platforms. Clips are 30–60s; the speed loss is acceptable and the look restores the pre-regression bar.
