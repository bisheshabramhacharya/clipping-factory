from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import clip_checks  # noqa: E402


class ParseScdetMsTests(unittest.TestCase):
    def test_ffmpeg4_log_line(self):
        self.assertEqual(
            clip_checks.parse_scdet_ms(
                "[scdet @ 0x0] lavfi.scd.score: 15.625, lavfi.scd.time: 4.2"
            ),
            4200,
        )

    def test_ffmpeg5_metadata_print(self):
        self.assertEqual(
            clip_checks.parse_scdet_ms(
                "[Parsed_metadata_2 @ 0x0] lavfi.scdet.time=12.345"
            ),
            12345,
        )

    def test_rejects_garbage(self):
        self.assertIsNone(clip_checks.parse_scdet_ms("frame=  100 fps=30"))
        self.assertIsNone(clip_checks.parse_scdet_ms("lavfi.scdet.time=abc"))
        self.assertIsNone(clip_checks.parse_scdet_ms("lavfi.scdet.time=-1.5"))
        self.assertIsNone(clip_checks.parse_scdet_ms("lavfi.scdet.time=nan"))


class CutViolationsTests(unittest.TestCase):
    def test_clean_clip_has_no_violations(self):
        self.assertEqual(clip_checks.cut_violations([10_000, 20_000], 30_000), [])

    def test_boundary_on_opening_cut_flags(self):
        v = clip_checks.cut_violations([300], 30_000)
        self.assertEqual(len(v), 1)
        self.assertIn("opens", v[0])

    def test_boundary_on_closing_cut_flags(self):
        v = clip_checks.cut_violations([29_800], 30_000)
        self.assertEqual(len(v), 1)
        self.assertIn("closes", v[0])

    def test_interior_boundary_is_not_a_cut_violation(self):
        # Mid-clip scene changes are the validator's business, not the
        # cut guard's: this check only covers open/close.
        self.assertEqual(clip_checks.cut_violations([15_000], 30_000), [])

    def test_just_outside_window_passes(self):
        self.assertEqual(
            clip_checks.cut_violations(
                [clip_checks.TRANSITION_HALF_MS + 1], 30_000
            ),
            [],
        )


class ClipCountTests(unittest.TestCase):
    def test_bounds(self):
        self.assertTrue(clip_checks.clip_count_ok(3, 1, 5))
        self.assertFalse(clip_checks.clip_count_ok(0, 1, 5))
        self.assertFalse(clip_checks.clip_count_ok(6, 1, 5))
        self.assertTrue(clip_checks.clip_count_ok(0, 0, 999))


class CaptionBandTests(unittest.TestCase):
    def test_parse_signalstats_both_separators(self):
        self.assertEqual(
            clip_checks.parse_signalstats_y(
                "lavfi.signalstats.YAVG=31.2 lavfi.signalstats.YHIGH=200 "
                "lavfi.signalstats.YLOW=16"
            ),
            (31.2, 184.0),
        )
        self.assertEqual(
            clip_checks.parse_signalstats_y("YAVG:22.0 YHIGH:180 YLOW:20"),
            (22.0, 160.0),
        )
        self.assertEqual(clip_checks.parse_signalstats_y("nothing"), (None, None))

    def test_flat_black_band_fails(self):
        self.assertFalse(clip_checks.caption_band_ok(0.0, 0.0))
        self.assertFalse(clip_checks.caption_band_ok(111.0, 5.0))

    def test_text_like_band_passes(self):
        self.assertTrue(clip_checks.caption_band_ok(111.0, 198.0))


if __name__ == "__main__":
    unittest.main()
