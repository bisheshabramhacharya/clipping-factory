"""Pure helpers for rendered-clip assertions (evals/verify_clip_quality.sh).

Kept dependency-free and importable so evals/tests can cover the judgment
logic without any media: the shell side shells out to ffmpeg/ffprobe for
pixels, these functions decide what the numbers mean.
"""

from __future__ import annotations

import math
import re

# Half-width of the transition window around a detected scene boundary,
# mirroring TRANSITION_HALF_MS in src/validate.rs: a cut inside this window
# lands on the crossfade itself.
TRANSITION_HALF_MS = 500


def parse_scdet_ms(line: str) -> int | None:
    """Parse one scdet detection line into milliseconds.

    ffmpeg >=5 reports ``lavfi.scdet.time=12.34`` (metadata=print) or
    ``lavfi.scdet.time: 12.34`` (the filter's own log line); 4.x names the
    same key ``lavfi.scd.time``. Same contract as media.rs.
    """
    for key in ("lavfi.scdet.time", "lavfi.scd.time"):
        i = line.find(key)
        if i >= 0:
            value = line[i + len(key):]
            token = value.lstrip("=:").split()
            if not token:
                return None
            try:
                secs = float(token[0])
            except ValueError:
                return None
            if math.isfinite(secs) and secs >= 0.0:
                return round(secs * 1000)
    return None


def cut_violations(
    boundaries_ms: list[int], duration_ms: int, half_ms: int = TRANSITION_HALF_MS
) -> list[str]:
    """Name every detected boundary that lands within +/-half_ms of the
    clip's opening or closing cut — the crossfade the validator promised
    not to open or close on.
    """
    violations = []
    for b in sorted(set(boundaries_ms)):
        if abs(b - 0) <= half_ms:
            violations.append(f"opens inside a scene transition at {b}ms")
        if abs(b - duration_ms) <= half_ms:
            violations.append(f"closes inside a scene transition at {b}ms")
    return violations


def clip_count_ok(ready: int, minimum: int, maximum: int) -> bool:
    """Ready-clip count sits inside the expected bounds (0 allowed —
    a valid run can honestly produce nothing)."""
    return minimum <= ready <= maximum


# metadata=print emits `lavfi.signalstats.YAVG=28.4` (and `:` on some builds)
_SAVG = re.compile(r"YAVG[=:]([\d.]+)")
_SHIGH = re.compile(r"YHIGH[=:]([\d.]+)")
_SLOW = re.compile(r"YLOW[=:]([\d.]+)")


def caption_band_ok(yavg: float, yspread: float, floor_avg: float = 10.0,
                    floor_spread: float = 60.0) -> bool:
    """A burned caption band shows contrast: blurred-pad video stays soft,
    text pushes the YLOW..YHIGH luma range wide open. Floors are generous —
    the assert is 'something bright and structured is there', not OCR."""
    return yavg > floor_avg and yspread > floor_spread


def parse_signalstats_y(stderr: str) -> tuple[float | None, float | None]:
    """Pull (YAVG, YHIGH-YLOW spread) out of signalstats frame metadata.
    ffmpeg 4.x signalstats does not emit YDEV, so spread stands in."""
    def grab(pattern):
        m = pattern.search(stderr)
        return float(m.group(1)) if m else None

    avg, high, low = grab(_SAVG), grab(_SHIGH), grab(_SLOW)
    spread = high - low if (high is not None and low is not None) else None
    return avg, spread
