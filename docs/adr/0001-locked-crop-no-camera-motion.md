# Locked crop: the camera never moves

Face-cropped clips previously panned a 1080-wide window along piecewise-linear keyframes following the face track. The pan was deliberate — commit `8042946` even reverted a "calm the pan" fix to restore the livelier motion — but rendered output reads as a camera that won't sit still, and the user wants it gone entirely.

Decision: a face-cropped clip picks ONE crop position — the dominant face's location across the whole clip — and holds it for the full duration. No keyframes, no easing, no dead band, no pan clamp. The face-track machinery reduces to choosing a single static `cx`. If the speaker leans out of frame, we accept the drift rather than reintroduce motion.

Clips whose dominant face is absent at the opening fall back to BlurPad rather than freezing the crop on the face's future position (the reverted `26e7e57` guard returns as the "opening face gate").
