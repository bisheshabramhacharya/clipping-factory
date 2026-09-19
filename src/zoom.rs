//! Zoom cuts (Spec #54) — subtle punch-in/out on emphasis beats, opt-in
//! per clip and off by default.
//!
//! Beats come from two signals the pipeline already produces:
//! - the episode's [`EnergyProfile`]: per-second loudness buckets whose
//!   z-score clears [`ENERGY_BEAT_Z`], one beat per loud burst;
//! - the caption emphasis picker: [`crate::captions::pick_emphasis`] run on
//!   each caption page of the clip's (edited) caption words, so a punch-in
//!   can land on the word the captions themselves spotlight.
//!
//! Auto-cut edits land first: every beat is mapped through the clip's
//! removal list onto the post-cut output timeline (beats inside a removed
//! span are dropped — zooming on a deleted "um" is nonsense), so the key
//! times stored on the ClipRecord line up with what the rendered file
//! actually plays.
//!
//! The zoom is a post-scale inside the Locked crop — ADR-0001 stands: the
//! crop position never moves; only magnification breathes in and settles
//! back to exactly 1.0×, so between beats there is no net motion.
//! Amplitude is small and beats are sparse (content-driven, never
//! periodic), which keeps PRD §11.3's "no repeated punch-in zoom pattern"
//! intact. FaceCrop only — a BlurPad composite has no locked crop to zoom
//! inside of.

use crate::domain::{CutSpan, Word, ZoomKey};
use crate::energy::EnergyProfile;

/// Peak magnification at a beat — a whisper of a punch-in.
pub const ZOOM_PEAK: f32 = 1.07;
/// Ramp in ahead of the beat and settle back out, in output-timeline ms.
pub const ZOOM_RISE_MS: u64 = 350;
pub const ZOOM_FALL_MS: u64 = 550;
/// Two beats closer than this collapse into the stronger one.
pub const MIN_BEAT_GAP_MS: u64 = 2_500;
/// Hard cap per clip — the punch-in stays an accent, not a pattern.
pub const MAX_BEATS: usize = 5;
/// A per-second energy bucket qualifies as a beat at this z-score or above.
const ENERGY_BEAT_Z: f32 = 1.5;

/// Plan the zoom keyframes for one clip. `words` are the clip's caption
/// words in absolute source ms (the same list captions are built from —
/// edits included). `removals` are the clip's merged auto-cut removals in
/// absolute ms; `out_dur_ms` is the clip's post-cut duration. The returned
/// keys are in output-timeline ms: always start at `(0, 1.0)`, peak at
/// `ZOOM_PEAK` on each selected beat, and return to `1.0` by the end, so a
/// render between beats is pixel-identical to no zoom.
pub fn plan(
    clip_start_ms: u64,
    clip_end_ms: u64,
    words: &[Word],
    energy: Option<&EnergyProfile>,
    removals: &[CutSpan],
    out_dur_ms: u64,
) -> Vec<ZoomKey> {
    let beats = select_beats(
        clip_start_ms,
        clip_end_ms,
        words,
        energy,
        removals,
        out_dur_ms,
    );
    keyframes(beats, out_dur_ms)
}

/// One candidate beat: absolute source ms and a strength used to arbitrate
/// overlap and the per-clip cap.
struct Beat {
    t_ms: u64,
    strength: f32,
}

/// Pick the surviving beats and move them onto the output timeline.
fn select_beats(
    clip_start_ms: u64,
    clip_end_ms: u64,
    words: &[Word],
    energy: Option<&EnergyProfile>,
    removals: &[CutSpan],
    out_dur_ms: u64,
) -> Vec<u64> {
    let mut candidates: Vec<Beat> = Vec::new();
    if let Some(profile) = energy {
        candidates.extend(energy_beats(profile, clip_start_ms, clip_end_ms));
    }
    candidates.extend(emphasis_beats(words));

    // Sort strongest-first (earlier time wins ties), then walk the ranking
    // keeping a beat only if it clears the gap from every already-kept one.
    candidates.sort_by(|a, b| {
        b.strength
            .partial_cmp(&a.strength)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.t_ms.cmp(&b.t_ms))
    });
    let mut kept: Vec<u64> = Vec::new();
    for beat in candidates {
        if kept.len() >= MAX_BEATS {
            break;
        }
        if inside_removal(beat.t_ms, removals) {
            continue;
        }
        let out_ms =
            crate::autocut::map_to_output(beat.t_ms, removals).saturating_sub(clip_start_ms);
        if out_ms >= out_dur_ms {
            continue;
        }
        if kept.iter().all(|t| out_ms.abs_diff(*t) >= MIN_BEAT_GAP_MS) {
            kept.push(out_ms);
        }
    }
    kept.sort_unstable();
    kept
}

/// True when `t` falls strictly inside a removed span (a point on a cut
/// boundary survives — it lands on a frame the keep list still contains).
fn inside_removal(t_ms: u64, removals: &[CutSpan]) -> bool {
    removals
        .iter()
        .any(|r| t_ms > r.start_ms && t_ms < r.end_ms)
}

/// Per-second loudness bursts inside the clip, one beat per contiguous run
/// over the z threshold, at the run's loudest bucket (bucket center).
fn energy_beats(profile: &EnergyProfile, clip_start_ms: u64, clip_end_ms: u64) -> Vec<Beat> {
    let db = &profile.per_second_db;
    if db.len() < 2 {
        return Vec::new();
    }
    let mean = db.iter().sum::<f32>() / db.len() as f32;
    let var = db.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / db.len() as f32;
    let std = var.sqrt();
    if std < 1.0 {
        return Vec::new(); // flat audio: no signal to exploit
    }
    let lo = ((clip_start_ms / 1000) as usize).min(db.len());
    let hi = (clip_end_ms.div_ceil(1000) as usize).min(db.len());

    let mut beats = Vec::new();
    let mut run_peak: Option<(usize, f32)> = None;
    for (i, v) in db.iter().enumerate().take(hi).skip(lo) {
        let z = (v - mean) / std;
        if z >= ENERGY_BEAT_Z {
            match run_peak {
                Some((_, pz)) if z <= pz => {}
                _ => run_peak = Some((i, z)),
            }
        } else if let Some((peak_i, peak_z)) = run_peak.take() {
            beats.push(Beat {
                t_ms: peak_i as u64 * 1000 + 500,
                strength: peak_z,
            });
        }
    }
    if let Some((peak_i, peak_z)) = run_peak {
        beats.push(Beat {
            t_ms: peak_i as u64 * 1000 + 500,
            strength: peak_z,
        });
    }
    beats
}

/// The caption emphasis picker's chosen word in each page — its onset is a
/// beat (a punch-in lands on the word the captions spotlight anyway).
fn emphasis_beats(words: &[Word]) -> Vec<Beat> {
    crate::captions::paginate_impact(words)
        .iter()
        .filter(|page| !page.is_empty())
        .map(|page| {
            let i = crate::captions::pick_emphasis(page);
            Beat {
                t_ms: page[i].start_ms,
                strength: 1.0,
            }
        })
        .collect()
}

/// Expand beats into keyframes: hold 1.0, ramp to the peak on the beat,
/// settle back to 1.0 — bookended by (0, 1.0) and (out_dur, 1.0) so the
/// expression is flat outside every bump.
fn keyframes(beats: Vec<u64>, out_dur_ms: u64) -> Vec<ZoomKey> {
    if beats.is_empty() || out_dur_ms == 0 {
        return Vec::new();
    }
    let mut keys = vec![ZoomKey { t_ms: 0, z: 1.0 }];
    for t in beats {
        keys.push(ZoomKey {
            t_ms: t.saturating_sub(ZOOM_RISE_MS),
            z: 1.0,
        });
        keys.push(ZoomKey {
            t_ms: t,
            z: ZOOM_PEAK,
        });
        keys.push(ZoomKey {
            t_ms: t.saturating_add(ZOOM_FALL_MS).min(out_dur_ms),
            z: 1.0,
        });
    }
    if keys.last().map(|k| k.t_ms).unwrap_or(0) < out_dur_ms {
        keys.push(ZoomKey {
            t_ms: out_dur_ms,
            z: 1.0,
        });
    }
    // Equal times can collide at the clip edges (a beat at t < RISE clamps
    // its rise to 0); the later key wins so the intended shape survives.
    keys.sort_by_key(|k| k.t_ms);
    keys.dedup_by_key(|k| k.t_ms);
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Word;

    fn word(text: &str, start_ms: u64, end_ms: u64) -> Word {
        Word {
            text: text.into(),
            start_ms,
            end_ms,
            p: 0.9,
        }
    }

    /// A quiet 120 s profile with loud bursts at the given second indexes.
    fn profile_with_bursts(bursts: &[usize]) -> EnergyProfile {
        let mut db = vec![-40.0f32; 120];
        for &i in bursts {
            db[i] = -15.0;
        }
        EnergyProfile { per_second_db: db }
    }

    #[test]
    fn energy_peaks_become_beats_on_the_output_timeline() {
        // Clip 30–60 s of a profile spiking at 35 s and 50 s.
        let energy = profile_with_bursts(&[35, 50]);
        let keys = plan(30_000, 60_000, &[], Some(&energy), &[], 30_000);
        let peaks: Vec<u64> = keys
            .iter()
            .filter(|k| k.z == ZOOM_PEAK)
            .map(|k| k.t_ms)
            .collect();
        assert_eq!(peaks, vec![5_500, 20_500]);
    }

    #[test]
    fn emphasis_words_become_beats() {
        // Two Impact pages (a ≥600 ms pause breaks them); "marketing" and
        // "stuff" are the substantial content words the picker chooses.
        let words = vec![
            word("marketing", 10_000, 10_400),
            word("wins", 10_400, 10_700),
            word("big", 10_700, 10_900),
            word("stuff", 14_000, 14_400),
            word("here", 14_400, 14_700),
        ];
        let keys = plan(9_000, 20_000, &words, None, &[], 11_000);
        let peaks: Vec<u64> = keys
            .iter()
            .filter(|k| k.z == ZOOM_PEAK)
            .map(|k| k.t_ms)
            .collect();
        assert_eq!(peaks, vec![1_000, 5_000]);
    }

    #[test]
    fn beats_inside_removals_drop_and_survivors_shift_to_the_post_cut_timeline() {
        let energy = profile_with_bursts(&[35, 40, 50]);
        // Cut 40–45 s out of clip 30–60 s: the 40 s beat is removed, the
        // 50 s beat lands at 20.5 s − 5 s = 15.5 s on the output timeline.
        let removals = vec![CutSpan {
            start_ms: 40_000,
            end_ms: 45_000,
        }];
        let keys = plan(30_000, 60_000, &[], Some(&energy), &removals, 25_000);
        let peaks: Vec<u64> = keys
            .iter()
            .filter(|k| k.z == ZOOM_PEAK)
            .map(|k| k.t_ms)
            .collect();
        assert_eq!(peaks, vec![5_500, 15_500]);
    }

    #[test]
    fn beats_are_capped_and_spaced() {
        // Ten loud bursts inside the clip: at most MAX_BEATS survive, all
        // at least MIN_BEAT_GAP_MS apart, strongest first (later spikes are
        // equally strong, so the earliest win on ties).
        let energy = profile_with_bursts(&[32, 34, 36, 38, 40, 42, 44, 46, 48, 50]);
        let keys = plan(30_000, 60_000, &[], Some(&energy), &[], 30_000);
        let peaks: Vec<u64> = keys
            .iter()
            .filter(|k| k.z == ZOOM_PEAK)
            .map(|k| k.t_ms)
            .collect();
        assert!(peaks.len() <= MAX_BEATS, "{peaks:?}");
        assert_eq!(peaks.len(), MAX_BEATS, "{peaks:?}");
        for pair in peaks.windows(2) {
            assert!(pair[1] - pair[0] >= MIN_BEAT_GAP_MS, "{peaks:?}");
        }
        assert_eq!(peaks, vec![2_500, 6_500, 10_500, 14_500, 18_500]);
    }

    #[test]
    fn keyframes_start_and_end_at_rest() {
        let energy = profile_with_bursts(&[40]);
        let keys = plan(30_000, 60_000, &[], Some(&energy), &[], 30_000);
        assert_eq!(keys.first().unwrap(), &ZoomKey { t_ms: 0, z: 1.0 });
        assert_eq!(keys.last().unwrap().z, 1.0);
        assert!(keys.last().unwrap().t_ms <= 30_000);
        // One bump: rest, rise start, peak, fall end, tail rest.
        assert_eq!(keys.len(), 3 + 2);
        assert_eq!(keys[2].z, ZOOM_PEAK);
        assert_eq!(keys[2].t_ms, 10_500);
    }

    #[test]
    fn flat_energy_and_no_words_means_no_zoom() {
        let flat = EnergyProfile {
            per_second_db: vec![-22.0; 60],
        };
        assert!(plan(0, 60_000, &[], Some(&flat), &[], 60_000).is_empty());
        assert!(plan(0, 60_000, &[], None, &[], 60_000).is_empty());
    }

    #[test]
    fn every_second_of_loudness_is_one_beat_not_per_bucket() {
        // A 5 s loud run produces a single beat at its loudest bucket.
        let mut energy = profile_with_bursts(&[]);
        for (i, v) in energy
            .per_second_db
            .iter_mut()
            .enumerate()
            .take(45)
            .skip(40)
        {
            *v = if i == 42 { -12.0 } else { -15.0 };
        }
        let keys = plan(30_000, 60_000, &[], Some(&energy), &[], 30_000);
        let peaks: Vec<u64> = keys
            .iter()
            .filter(|k| k.z == ZOOM_PEAK)
            .map(|k| k.t_ms)
            .collect();
        assert_eq!(peaks, vec![12_500]);
    }
}
