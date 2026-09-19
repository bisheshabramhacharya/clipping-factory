//! Clip rendering (PRD §11–12), in two passes:
//!
//! 1. [`render_base_clip`] — one continuous source interval → framed
//!    vertical H.264/AAC MP4 **without captions**, sized by [`output_size`]
//!    (native crop window, capped at 1080×1920 — ADR-0002). This is the
//!    expensive pass (decode, scale, blur or face-tracked crop, encode). The
//!    base is kept on disk so caption styling can change later without
//!    re-doing it.
//! 2. [`burn_captions`] — base MP4 + generated ASS → final captioned MP4.
//!    Fast: the video is re-encoded at output size with only the subtitle
//!    filter, and the audio stream is copied bit-for-bit.
//!
//! Layouts (house style, §11.2/11.3; ADR-0004 for the two-person forms):
//! - BlurPad:  source centered over a blurred, darkened copy of itself.
//! - FaceCrop: vertical crop locked on the dominant face — a constant x.
//!   Manifests written before ADR-0001 may carry several keyframes; those
//!   still render as a piecewise-linear x(t) crop expression.
//! - Split:    two stacked panels, each a locked crop on one face — the
//!   wide two-shot's answer.
//! - SpeakerCrop: the FaceCrop window, but it hard-cuts between faces at
//!   speaker-turn boundaries — a cut, never a pan.

use crate::config::Config;
use crate::domain::{CropKey, CutSpan, LayoutPlan, SourceInfo, ZoomKey};
use crate::util::run_streaming;
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

/// Output size ceilings (ADR-0002). A clip renders at its native crop
/// window capped here — pixels are never stretched up. `captions.rs`
/// authors its ASS geometry against this same canvas and scales to the
/// clip's real size, so a resolution change lands here and nowhere else.
pub const OUT_W: u32 = 1080;
pub const OUT_H: u32 = 1920;

/// The clip's rendered size (ADR-0002): the native crop window, capped at
/// OUT_W×OUT_H and even-dimensioned for yuv420p — never upscaled.
///
/// - FaceCrop / SpeakerCrop: `source_h × 9/16` wide × `source_h` tall,
///   capped. (SpeakerCrop is the same window; only its x behaves
///   differently.)
/// - Split: same window — each half-height panel holds a face-anchored
///   crop of matching aspect.
/// - BlurPad: the largest 9:16 canvas inscribed in the source, capped.
pub fn output_size(source: &SourceInfo, layout: &LayoutPlan) -> (u32, u32) {
    let (sw, sh) = (source.width as f64, source.height as f64);
    match layout {
        LayoutPlan::FaceCrop { .. } | LayoutPlan::SpeakerCrop { .. }
            if face_window_fits(source) =>
        {
            let h = source.height.min(OUT_H) & !1;
            let w = even_round(h as f64 * 9.0 / 16.0).min(OUT_W);
            (w, h)
        }
        LayoutPlan::Split { .. } if split_window_fits(source) => {
            let h = source.height.min(OUT_H) & !1;
            let w = even_round(h as f64 * 9.0 / 16.0).min(OUT_W);
            (w, h)
        }
        // BlurPad, or a crop window that cannot fit the source.
        _ => {
            let h = even_floor(sh.min(sw * 16.0 / 9.0)).min(OUT_H);
            let w = even_round(h as f64 * 9.0 / 16.0)
                .min(even_floor(sw))
                .min(OUT_W);
            (w, h)
        }
    }
}

/// True when the native 9:16 crop window fits inside the source frame.
/// Integer compare, so portrait-ish sources degrade to BlurPad exactly.
fn face_window_fits(source: &SourceInfo) -> bool {
    source.width as u64 * 16 >= source.height as u64 * 9
}

/// True when each Split panel's 9:8 source window fits — the source must
/// be at least 9:8 wide to give each face a full-height column.
fn split_window_fits(source: &SourceInfo) -> bool {
    source.width as u64 * 8 >= source.height as u64 * 9
}

/// Nearest even value (yuv420p needs even dimensions).
fn even_round(v: f64) -> u32 {
    ((v / 2.0).round() as u32) * 2
}

/// Largest even value not exceeding `v` — never rounds past the source.
fn even_floor(v: f64) -> u32 {
    (v as u32) & !1
}

/// Render the framed, uncaptioned base clip from the source video.
/// `keeps` is auto-cut's keep list: empty or a single span renders the one
/// continuous excerpt exactly as before; two or more spans go through a
/// trim+concat stage that lifts the cut out of the stream before framing.
/// `zoom` is zoom cuts' keyframe list on the post-cut timeline — a punch
/// inside the Locked crop, applied only by the FaceCrop path.
/// (The argument list mirrors the render inputs one-to-one on purpose.)
#[allow(clippy::too_many_arguments)]
pub async fn render_base_clip<F>(
    cfg: &Config,
    src: &Path,
    source: &SourceInfo,
    layout: &LayoutPlan,
    start_ms: u64,
    end_ms: u64,
    keeps: &[CutSpan],
    zoom: &[ZoomKey],
    end_card: bool,
    bar: Option<&str>,
    hook: Option<HookSpec<'_>>,
    out_path: &Path,
    cancel: &CancellationToken,
    mut on_progress: F,
) -> Result<()>
where
    F: FnMut(f32),
{
    // Speaker-crop keys live on the clip's source timeline; when auto-cut
    // has removed spans, move each cut onto the post-cut output timeline —
    // a boundary inside a removal lands exactly on the cut seam.
    let mapped_layout;
    let layout = match (layout, keeps.len() > 1) {
        (LayoutPlan::SpeakerCrop { keyframes }, true) => {
            mapped_layout = LayoutPlan::SpeakerCrop {
                keyframes: crop_keys_to_output(keyframes, start_ms, keeps),
            };
            &mapped_layout
        }
        _ => layout,
    };

    // One surviving span only needs the input window re-aimed at it; several
    // spans keep the full clip read and let the graph pick them out.
    let (in_start_ms, in_dur_ms) = if keeps.len() == 1 {
        (keeps[0].start_ms, keeps[0].len_ms())
    } else {
        (start_ms, end_ms.saturating_sub(start_ms))
    };
    let dur_s = in_dur_ms as f64 / 1000.0;
    // Progress is measured against what the file will contain, not what was
    // read — the concat path discards removed time on the way through.
    let clip_dur_ms: u64 = if keeps.len() > 1 {
        keeps.iter().map(|k| k.len_ms()).sum()
    } else {
        in_dur_ms
    };
    // The card tail only exists when the bundled display font resolves —
    // without it the toggle silently renders plain.
    let card_font = if end_card { end_card_font(cfg) } else { None };
    let out_dur_ms = clip_dur_ms + card_font.as_ref().map(|_| END_CARD_MS).unwrap_or(0);
    let out_dur_s = out_dur_ms as f64 / 1000.0;
    let card = card_font.as_deref();
    // The hook title's wrapped lines and resolved face are owned here and
    // borrowed by the garnish for the graph build below.
    let hook_lines = hook
        .map(|h| wrap_hook_title(h.headline, h.caps))
        .unwrap_or_default();
    let hook_file = hook.and_then(|h| hook_font_file(cfg, h.font));
    let garnish = Garnish {
        card,
        bar,
        hook: hook.map(|h| HookTitle {
            lines: &hook_lines,
            fontfile: hook_file.as_deref(),
            font: h.face,
        }),
    };
    let graph = if keeps.len() > 1 {
        build_cut_graph(
            source,
            layout,
            None,
            CutSpec {
                keeps,
                origin_ms: start_ms,
            },
            zoom,
            clip_dur_ms,
            garnish,
        )
    } else {
        build_graph(source, layout, None, zoom, clip_dur_ms, garnish)
    };

    let mut args: Vec<String> = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-ss".into(),
        format!("{:.3}", in_start_ms as f64 / 1000.0),
        "-t".into(),
        format!("{:.3}", dur_s),
        "-i".into(),
        src.to_string_lossy().into_owned(),
        "-filter_complex".into(),
        graph,
        "-map".into(),
        "[v]".into(),
        "-map".into(),
        "[a]".into(),
    ];
    args.extend(video_encode_args());
    args.extend([
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "160k".into(),
        "-ar".into(),
        "48000".into(),
        "-movflags".into(),
        "+faststart".into(),
    ]);
    // Preserve source frame rate when practical, otherwise 30 fps (PRD §11.1).
    if !(20.0..=60.0).contains(&source.fps) {
        args.push("-r".into());
        args.push("30".into());
    }
    args.push("-progress".into());
    args.push("pipe:1".into());
    args.push(out_path.to_string_lossy().into_owned());

    run_ffmpeg_with_progress(cfg, &args, out_dur_s, cancel, &mut on_progress)
        .await
        .map_err(|e| {
            if e.to_string().contains("cancelled") {
                e
            } else {
                anyhow!("Render failed. {}", e)
            }
        })?;
    ensure_nontrivial(out_path, "Render produced an empty file. Retry this clip.").await
}

/// Burn ASS captions onto an already-framed base clip. The video is
/// re-encoded (subtitle filter only); the audio stream is copied.
pub async fn burn_captions<F>(
    cfg: &Config,
    base: &Path,
    ass_path: &Path,
    out_path: &Path,
    dur_ms: u64,
    cancel: &CancellationToken,
    mut on_progress: F,
) -> Result<()>
where
    F: FnMut(f32),
{
    let dur_s = dur_ms as f64 / 1000.0;
    let subs = subtitles_filter(cfg.fonts_dir.as_deref(), ass_path);
    let mut args: Vec<String> = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        base.to_string_lossy().into_owned(),
        "-vf".into(),
        subs,
    ];
    args.extend(video_encode_args());
    args.extend([
        "-c:a".into(),
        "copy".into(),
        "-movflags".into(),
        "+faststart".into(),
        "-progress".into(),
        "pipe:1".into(),
        out_path.to_string_lossy().into_owned(),
    ]);

    run_ffmpeg_with_progress(cfg, &args, dur_s, cancel, &mut on_progress)
        .await
        .map_err(|e| {
            if e.to_string().contains("cancelled") {
                e
            } else {
                anyhow!("Caption burn failed. {}", e)
            }
        })?;
    ensure_nontrivial(
        out_path,
        "Caption burn produced an empty file. Retry this clip.",
    )
    .await
}

async fn run_ffmpeg_with_progress<F>(
    cfg: &Config,
    args: &[String],
    dur_s: f64,
    cancel: &CancellationToken,
    on_progress: &mut F,
) -> Result<()>
where
    F: FnMut(f32),
{
    let dur_us = dur_s * 1_000_000.0;
    run_streaming(&cfg.ffmpeg, args, cancel, |is_err, line| {
        if !is_err {
            if let Some(us) = line
                .strip_prefix("out_time_ms=")
                .and_then(|v| v.parse::<f64>().ok())
            {
                if dur_us > 0.0 {
                    on_progress((us / dur_us).clamp(0.0, 1.0) as f32);
                }
            }
        }
    })
    .await
}

/// Shared output settings for both passes (PRD §11.1): always libx264 at
/// high quality, on every platform (ADR-0003). The VideoToolbox hardware
/// encoder is faster but its quality slider doesn't track CRF semantics, and
/// clips visibly degraded at -q:v 60.
fn video_encode_args() -> Vec<String> {
    vec![
        "-c:v".into(),
        "libx264".into(),
        "-crf".into(),
        "17".into(),
        "-preset".into(),
        "fast".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
    ]
}

/// The output must exist and be non-trivial.
async fn ensure_nontrivial(path: &Path, msg: &str) -> Result<()> {
    let size = tokio::fs::metadata(path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if size < 10_000 {
        return Err(anyhow!("{}", msg));
    }
    Ok(())
}

/// The `ass=` subtitle filter clause, with optional bundled-fonts directory.
pub fn subtitles_filter(fonts_dir: Option<&Path>, ass_path: &Path) -> String {
    let ass = ff_escape_str(&ass_path.to_string_lossy());
    let fonts = fonts_dir
        .map(|d| format!(":fontsdir='{}'", ff_escape_str(&d.to_string_lossy())))
        .unwrap_or_default();
    format!("ass='{}'{}", ass, fonts)
}

/// The stream labels a graph stage reads from (`in_*`) and writes to
/// (`out_*`). With an end card the outs are `v0`/`a0` for the tail's concat;
/// otherwise they're the final `[v]`/`[a]`.
struct Pads<'a> {
    in_v: &'a str,
    in_a: &'a str,
    out_v: &'a str,
    out_a: &'a str,
}

/// Opt-in garnish applied to a base render: the end card tail (font must
/// resolve or the card is absent), the progress bar (accent hex or None),
/// and the hook title (opening title card, None unless the toggle is on
/// and the headline wrapped to at least one line).
#[derive(Clone, Copy, Default)]
struct Garnish<'a> {
    card: Option<&'a Path>,
    bar: Option<&'a str>,
    hook: Option<HookTitle<'a>>,
}

/// What the hook title needs that the render already knows: the headline
/// text plus the caption styling to match (font family and caps voice).
#[derive(Clone, Copy)]
pub struct HookSpec<'a> {
    /// The clip's headline — wrapped and case-folded at render time.
    pub headline: &'a str,
    /// The resolved caption font family, e.g. "Inter".
    pub font: &'a str,
    /// The style's face name for that family (fontconfig fallback), e.g.
    /// "Inter ExtraBold" under Impact — see `CaptionStyle::face`.
    pub face: &'a str,
    /// True when the caption style renders its display text uppercase.
    pub caps: bool,
}

/// A hook title ready to draw: `lines` are the wrapped title rows, and the
/// face is the bundled `fontfile` when the caption family is vendored or
/// the style's `font` face name (fontconfig) when it is not.
#[derive(Clone, Copy)]
struct HookTitle<'a> {
    lines: &'a [String],
    fontfile: Option<&'a Path>,
    font: &'a str,
}

fn build_graph(
    source: &SourceInfo,
    layout: &LayoutPlan,
    subs: Option<&str>,
    zoom: &[ZoomKey],
    dur_ms: u64,
    garnish: Garnish<'_>,
) -> String {
    let body = graph_body(source, layout, subs, zoom, dur_ms, ("0:v", "0:a"), garnish);
    match garnish.card {
        Some(font) => format!("{body};{}", end_card_tail(source, layout, font)),
        None => body,
    }
}

/// The framing body plus an optional end-card tail via intermediate pads.
fn graph_body(
    source: &SourceInfo,
    layout: &LayoutPlan,
    subs: Option<&str>,
    zoom: &[ZoomKey],
    dur_ms: u64,
    inputs: (&str, &str),
    garnish: Garnish<'_>,
) -> String {
    let (in_v, in_a) = inputs;
    let pads = if garnish.card.is_some() {
        Pads {
            in_v,
            in_a,
            out_v: "v0",
            out_a: "a0",
        }
    } else {
        Pads {
            in_v,
            in_a,
            out_v: "v",
            out_a: "a",
        }
    };
    build_graph_from(source, layout, subs, zoom, dur_ms, pads, garnish)
}

/// The framing graph, reading from named pads instead of input 0 — the cut
/// variant feeds it the concat output instead of the raw source.
fn build_graph_from(
    source: &SourceInfo,
    layout: &LayoutPlan,
    subs: Option<&str>,
    zoom: &[ZoomKey],
    dur_ms: u64,
    pads: Pads<'_>,
    garnish: Garnish<'_>,
) -> String {
    // Trailing subtitle step when burning in one pass; empty for base renders.
    let subs_step = subs.map(|s| format!("{},", s)).unwrap_or_default();
    let bar_step = garnish
        .bar
        .map(|hex| format!("{},", progress_bar_step(hex, dur_ms)))
        .unwrap_or_default();
    let Pads {
        in_v: vpad,
        in_a: apad,
        out_v: vout,
        out_a: aout,
    } = pads;
    let audio = audio_chain(dur_ms);
    let (w, h) = output_size(source, layout);
    let hook_step = hook_title_step(garnish.hook, dur_ms, w, h);
    let crop = match layout {
        LayoutPlan::FaceCrop { keyframes } | LayoutPlan::SpeakerCrop { keyframes }
            if face_window_fits(source) =>
        {
            Some(keyframes)
        }
        _ => None,
    };
    // Split renders on its own graph — two locked crops stacked, not one
    // crop following a position.
    if let (LayoutPlan::Split { top, bottom }, true) = (layout, split_window_fits(source)) {
        return split_graph(source, *top, *bottom, w, h, subs, &hook_step, vpad, apad);
    }
    match crop {
        None => format!(
            "[{vpad}]setpts=PTS-STARTPTS,split=2[bga][fga];\
             [bga]scale={w}:{h}:force_original_aspect_ratio=increase:force_divisible_by=2,\
             crop={w}:{h},gblur=sigma=26,eq=brightness=-0.14:saturation=0.8[bg];\
             [fga]scale={w}:{h}:force_original_aspect_ratio=decrease:force_divisible_by=2[fg];\
             [bg][fg]overlay=(W-w)/2:(H-h)/2,{subs}{hook}{bar_step}format=yuv420p[{vout}];\
             [{apad}]{audio}[{aout}]",
            vpad = vpad,
            apad = apad,
            w = w,
            h = h,
            subs = subs_step,
            audio = audio,
            vout = vout,
            aout = aout,
            hook = hook_step,
            bar_step = bar_step
        ),
        Some(keyframes) => {
            // Downscale only when the source is taller than the ceiling;
            // at or under it the native window is cropped directly — the
            // pixels pass through unresampled.
            let (scale_step, frame_w) = if source.height > OUT_H {
                let scaled_w =
                    even_round(source.width as f64 * h as f64 / source.height as f64) as u64;
                (format!("scale=-2:{h}:force_divisible_by=2,"), scaled_w)
            } else {
                (String::new(), source.width as u64)
            };
            if frame_w < w as u64 {
                // Shouldn't happen (face_window_fits ran above), but stay
                // safe — and BlurPad has no Locked crop for zoom to live in.
                return build_graph_from(
                    source,
                    &LayoutPlan::BlurPad,
                    subs,
                    &[],
                    dur_ms,
                    Pads {
                        in_v: vpad,
                        in_a: apad,
                        out_v: vout,
                        out_a: aout,
                    },
                    garnish,
                );
            }
            let expr = match layout {
                // SpeakerCrop: hard cuts at key times — value steps, never
                // interpolates (ADR-0004).
                LayoutPlan::SpeakerCrop { .. } => crop_x_expr_stepped(keyframes, frame_w, w as u64),
                _ => crop_x_expr(keyframes, frame_w, w as u64),
            };
            // Zoom cuts: a post-scale punch inside the locked window (the
            // crop's x never moves). zoompan re-scales a centered subregion
            // of the cropped frame back to output size; between keys the
            // expression is exactly 1.0, i.e. pixel-identical to no zoom.
            // fps must follow the source — zoompan defaults to 25 and would
            // otherwise retime the video out of sync with the audio.
            let zoom_step = if zoom.is_empty() || !(source.fps > 0.0 && source.fps.is_finite()) {
                String::new()
            } else {
                format!(
                    "zoompan=z='{}':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=1:s={w}x{h}:fps={:.3},",
                    zoom_z_expr(zoom),
                    source.fps,
                    w = w,
                    h = h,
                )
            };
            // Eye-line offset: a nonzero dy slides the cropped frame over
            // a blurred copy of the source (the underlay fills the vacated
            // band, same recipe as BlurPad). dy=0 keeps the bare crop,
            // pixel-identical to the framing before eye-line anchoring.
            if keyframes.iter().any(|k| k.dy != 0.0) {
                let y_expr = match layout {
                    LayoutPlan::SpeakerCrop { .. } => overlay_y_expr_stepped(keyframes, h as u64),
                    _ => overlay_y_expr(keyframes, h as u64),
                };
                format!(
                    "[{vpad}]setpts=PTS-STARTPTS,split=2[bga][fga];\
                     [bga]scale={w}:{h}:force_original_aspect_ratio=increase:force_divisible_by=2,\
                     crop={w}:{h},gblur=sigma=26,eq=brightness=-0.14:saturation=0.8[bg];\
                     [fga]{scale}crop={w}:{h}:x='{expr}':y=0[fg];\
                     [bg][fg]overlay=(W-w)/2:'{y}',{zoom}{subs}{hook}{bar_step}format=yuv420p[{vout}];\
                     [{apad}]{audio}[{aout}]",
                    vpad = vpad,
                    apad = apad,
                    w = w,
                    h = h,
                    scale = scale_step,
                    expr = expr,
                    y = y_expr,
                    zoom = zoom_step,
                    subs = subs_step,
                    hook = hook_step,
                    bar_step = bar_step,
                    audio = audio,
                    vout = vout,
                    aout = aout
                )
            } else {
                format!(
                    "[{vpad}]setpts=PTS-STARTPTS,{scale}crop={w}:{h}:x='{expr}':y=0,\
                     {zoom}{subs}{hook}{bar_step}format=yuv420p[{vout}];\
                     [{apad}]{audio}[{aout}]",
                    scale = scale_step,
                    vpad = vpad,
                    apad = apad,
                    w = w,
                    h = h,
                    expr = expr,
                    zoom = zoom_step,
                    subs = subs_step,
                    hook = hook_step,
                    bar_step = bar_step,
                    audio = audio,
                    vout = vout,
                    aout = aout
                )
            }
        }
    }
}

/// A multi-keep render: the spans to keep and the `-ss` point the input is
/// relative to.
struct CutSpec<'a> {
    keeps: &'a [CutSpan],
    origin_ms: u64,
}

/// Auto-cut graph: split the (already `-ss`-seeked) input into one branch per
/// kept span, trim each to its relative window, reset timestamps, and concat
/// the survivors into a single stream the normal framing graph then shapes.
fn build_cut_graph(
    source: &SourceInfo,
    layout: &LayoutPlan,
    subs: Option<&str>,
    cut: CutSpec<'_>,
    zoom: &[ZoomKey],
    dur_ms: u64,
    garnish: Garnish<'_>,
) -> String {
    let keeps = cut.keeps;
    let origin_ms = cut.origin_ms;
    let n = keeps.len();
    let mut g = String::new();
    g.push_str(&format!("[0:v]split={n}"));
    for i in 0..n {
        g.push_str(&format!("[cv{i}]"));
    }
    g.push(';');
    g.push_str(&format!("[0:a]asplit={n}"));
    for i in 0..n {
        g.push_str(&format!("[ca{i}]"));
    }
    g.push(';');
    for (i, k) in keeps.iter().enumerate() {
        let s = k.start_ms.saturating_sub(origin_ms) as f64 / 1000.0;
        let d = k.len_ms() as f64 / 1000.0;
        g.push_str(&format!(
            "[cv{i}]trim=start={s:.3}:duration={d:.3},setpts=PTS-STARTPTS[cvt{i}];\
             [ca{i}]atrim=start={s:.3}:duration={d:.3},asetpts=PTS-STARTPTS[cat{i}];"
        ));
    }
    for i in 0..n {
        g.push_str(&format!("[cvt{i}]"));
    }
    g.push_str(&format!("concat=n={n}:v=1:a=0[cvj];"));
    for i in 0..n {
        g.push_str(&format!("[cat{i}]"));
    }
    g.push_str(&format!("concat=n={n}:v=0:a=1[caj];"));
    g.push_str(&graph_body(
        source,
        layout,
        subs,
        zoom,
        dur_ms,
        ("cvj", "caj"),
        garnish,
    ));
    if let Some(font) = garnish.card {
        g.push(';');
        g.push_str(&end_card_tail(source, layout, font));
    }
    g
}

/// End-card length: a beat long enough to register as intentional, short
/// enough to never feel like a watermark wall.
const END_CARD_MS: u64 = 1200;

/// The bundled display face the card is typeset in. None when the fonts
/// directory or the face itself is missing — the caller treats that as
/// "no card" rather than letting drawtext fail the render.
fn end_card_font(cfg: &Config) -> Option<PathBuf> {
    let f = cfg.fonts_dir.as_deref()?.join("Inter-ExtraBold.ttf");
    f.is_file().then_some(f)
}

/// Opt-in progress bar: a thin accent-colored strip along the bottom edge
/// filling left-to-right over the clip's content duration. drawbox clamps
/// the width at the frame edge, so the appended card tail just holds it
/// full. `hex` is an "#RRGGBB" accent; only hex digits survive into the
/// filter value.
const BAR_H: u32 = 6;
fn progress_bar_step(hex: &str, dur_ms: u64) -> String {
    let digits: String = hex.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    format!(
        "drawbox=x=0:y='ih-{BAR_H}':w='trunc(iw*t/{dur:.3})':h={BAR_H}:color=0x{digits}@0.9:t=fill",
        dur = dur_ms as f64 / 1000.0,
    )
}

/// The hook title's on-screen budget: at most two lines of 22 characters,
/// the longest a shouting title card stays readable at a glance.
const HOOK_LINE_CHARS: usize = 22;
const HOOK_MAX_LINES: usize = 2;

/// Wrap a headline for the title card: greedy word wrap at 22 characters,
/// capped at two lines; an overflow earns a trailing ellipsis inside the
/// same budget. Empty input wraps to nothing — the title draws no-op.
fn wrap_hook_title(headline: &str, caps: bool) -> Vec<String> {
    let text = headline.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = if caps { text.to_uppercase() } else { text };
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split(' ') {
        if word.is_empty() {
            continue;
        }
        // A word alone wider than the budget is hard-split into chunks.
        let mut rest = word;
        loop {
            let cur_len = cur.chars().count();
            let room = if cur.is_empty() {
                HOOK_LINE_CHARS
            } else {
                HOOK_LINE_CHARS.saturating_sub(cur_len + 1)
            };
            let word_len = rest.chars().count();
            if word_len <= room {
                if !cur.is_empty() {
                    cur.push(' ');
                }
                cur.push_str(rest);
                break;
            }
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                continue;
            }
            if word_len <= HOOK_LINE_CHARS {
                cur = rest.to_string();
                break;
            }
            let cut: String = rest.chars().take(HOOK_LINE_CHARS).collect();
            let byte_len = cut.len();
            lines.push(cut);
            rest = &rest[byte_len..];
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.len() > HOOK_MAX_LINES {
        lines.truncate(HOOK_MAX_LINES);
        let last = lines.last_mut().unwrap();
        while last.chars().count() >= HOOK_LINE_CHARS {
            match last.rfind(' ') {
                Some(sp) => last.truncate(sp),
                None => {
                    last.pop();
                }
            }
        }
        last.push('…');
    }
    lines
}

/// The heaviest bundled face for a caption font family ("Inter" →
/// Inter-ExtraBold.ttf), resolved by scanning fonts_dir. None when the
/// family isn't vendored — the caller then typesets via the family name.
fn hook_font_file(cfg: &Config, family: &str) -> Option<PathBuf> {
    let dir = cfg.fonts_dir.as_deref()?;
    let want: String = family
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    let mut best: Option<(u8, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let raw = entry.file_name().to_string_lossy().to_lowercase();
        let Some(stem) = raw
            .strip_suffix(".ttf")
            .or_else(|| raw.strip_suffix(".otf"))
        else {
            continue;
        };
        let norm: String = stem.chars().filter(|c| c.is_alphanumeric()).collect();
        let Some(rest) = norm.strip_prefix(&want) else {
            continue;
        };
        let rank = match rest {
            "" | "regular" | "normal" | "roman" => 0,
            "thin" | "extralight" | "light" => 0,
            "medium" => 1,
            "semibold" | "demibold" => 2,
            "bold" => 3,
            "extrabold" | "ultrabold" | "black" | "heavy" => 4,
            _ => continue,
        };
        let better = match &best {
            Some((r, p)) => rank > *r || (rank == *r && entry.path() < *p),
            None => true,
        };
        if better {
            best = Some((rank, entry.path()));
        }
    }
    best.map(|(_, p)| p)
}

/// Escape text for a single-quoted drawtext `text=` value: `'` would end
/// the quoting (close-escape-reopen), `:` and `,` are filter separators,
/// `%` starts a drawtext expansion, `\` is an escape lead, and control
/// characters break the filter line — so they fold to a space.
fn drawtext_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push('/'),
            ':' => out.push_str("\\:"),
            ',' => out.push_str("\\,"),
            '%' => out.push_str("%%"),
            '\'' => out.push_str("'\\''"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// The hook-title drawtext chain: each wrapped line centered in the upper
/// safe zone (~14% down, clear of the ~60–70% caption band), dropped after
/// the opening beat — min(1.8 s, 25% of the clip) via `enable`. Empty or
/// absent titles emit no step at all.
fn hook_title_step(hook: Option<HookTitle>, dur_ms: u64, w: u32, h: u32) -> String {
    let Some(hook) = hook else {
        return String::new();
    };
    if hook.lines.is_empty() {
        return String::new();
    }
    let show_s = ((dur_ms as f64 / 1000.0) * 0.25).min(1.8);
    // A full 22-char caps line (~0.62 em/char) must stay inside ~85% of the
    // frame width; the black stroke follows at ~8% of the font size.
    let fs = (w as f64 * 0.062).max(16.0);
    let border = (fs * 0.08).max(1.0);
    let font = match hook.fontfile {
        Some(path) => format!("fontfile='{}'", ff_escape_str(&path.to_string_lossy())),
        None => format!("font='{}'", drawtext_escape(hook.font)),
    };
    let pitch = fs * 1.2;
    let mut step = String::new();
    for (i, line) in hook.lines.iter().enumerate() {
        step.push_str(&format!(
            "drawtext={font}:text='{text}':fontcolor=white:fontsize={fs:.0}:\
             borderw={border:.0}:bordercolor=black:x=(w-text_w)/2:y={y:.0}:\
             enable='between(t,0,{show_s:.2})',",
            text = drawtext_escape(line),
            y = h as f64 * 0.14 + i as f64 * pitch,
        ));
    }
    step
}

/// Extra output length the card adds, for progress/duration bookkeeping.
/// Zero when off or when the font is missing (the render then has no tail).
pub(crate) fn end_card_ms(cfg: &Config, on: bool) -> u64 {
    if on && end_card_font(cfg).is_some() {
        END_CARD_MS
    } else {
        0
    }
}

/// A generated tail appended after the clip: a 1.2 s near-black card with
/// the product line centered, fading in over 150 ms, plus a matching silent
/// stereo pad so the concat never drops the audio stream.
fn end_card_tail(source: &SourceInfo, layout: &LayoutPlan, font: &Path) -> String {
    let (w, h) = output_size(source, layout);
    let fps = if source.fps.is_finite() && source.fps > 0.0 {
        source.fps
    } else {
        30.0
    };
    let d = END_CARD_MS as f64 / 1000.0;
    // Two-line lockup sized off the frame width: "Made with" sits quiet above
    // a bold "Clipping Factory". The big line is ~16 glyphs at ~0.62em, so
    // 8.2% of the width keeps it inside the frame at any output size.
    let fs_big = w as f64 * 0.082;
    let fs_small = fs_big * 0.42;
    let gap = fs_big * 0.28;
    format!(
        "color=c=0x0B0B0F:s={w}x{h}:r={fps:.3}:d={d:.3},format=yuv420p[cv];\
         [cv]drawtext=fontfile='{font}':text='Made with':fontcolor=0xA8A8B0:\
         fontsize={fs_small:.0}:x=(w-text_w)/2:y=(h-text_h)/2-{off:.0},\
         drawtext=fontfile='{font}':text='Clipping Factory':fontcolor=0xF5F5F0:\
         fontsize={fs_big:.0}:x=(w-text_w)/2:y=(h-text_h)/2+{gap:.0},\
         fade=t=in:st=0:d=0.15[cardv];\
         anullsrc=r=48000:cl=stereo:d={d:.3}[carda];\
         [v0][cardv]concat=n=2:v=1:a=0[v];\
         [a0][carda]concat=n=2:v=0:a=1[a]",
        w = w,
        h = h,
        fps = fps,
        d = d,
        font = ff_escape_str(&font.to_string_lossy()),
        fs_small = fs_small,
        fs_big = fs_big,
        off = gap + fs_small * 0.7,
        gap = gap,
    )
}

/// Audio stage shared by both layouts: loudness-normalize to the
/// short-form convention (-16 LUFS integrated, -1.5 dB true peak) and add
/// 50 ms in / 80 ms out fades so a clip never opens or closes on a hard
/// sample edge. `dur_ms` is the OUTPUT duration — the concat path is
/// PTS-reset, so the tail fade keys off the joined length.
fn audio_chain(dur_ms: u64) -> String {
    let fade_out_s = dur_ms.saturating_sub(80) as f64 / 1000.0;
    format!(
        "asetpts=PTS-STARTPTS,loudnorm=I=-16:TP=-1.5:LRA=11,\
         aformat=channel_layouts=stereo:sample_rates=48000,\
         afade=t=in:st=0:d=0.05,afade=t=out:st={fade_out_s:.3}:d=0.08"
    )
}

/// Piecewise-linear x(t) between keyframes, clamped so the `crop_w`-wide
/// window stays inside the `frame_w`-wide (possibly scaled) frame.
/// `t` in the crop filter is the output timestamp in seconds (0 at clip start).
pub fn crop_x_expr(keyframes: &[CropKey], frame_w: u64, crop_w: u64) -> String {
    let max_x = frame_w.saturating_sub(crop_w) as f64;
    let px = |cx: f32| -> f64 {
        ((cx as f64) * frame_w as f64 - (crop_w as f64) / 2.0).clamp(0.0, max_x)
    };

    match keyframes.len() {
        0 => format!("{:.1}", max_x / 2.0),
        1 => format!("{:.1}", px(keyframes[0].cx)),
        _ => {
            // Innermost value: hold the last keyframe.
            let mut expr = format!("{:.1}", px(keyframes[keyframes.len() - 1].cx));
            for pair in keyframes.windows(2).rev() {
                let (a, b) = (&pair[0], &pair[1]);
                let (t0, t1) = (a.t_ms as f64 / 1000.0, b.t_ms as f64 / 1000.0);
                let (x0, x1) = (px(a.cx), px(b.cx));
                if t1 <= t0 {
                    continue;
                }
                expr = format!(
                    "if(lt(t\\,{t1:.3})\\,{x0:.1}+({x1:.1}-{x0:.1})*(t-{t0:.3})/{dt:.3}\\,{rest})",
                    t1 = t1,
                    x0 = x0,
                    x1 = x1,
                    t0 = t0,
                    dt = t1 - t0,
                    rest = expr
                );
            }
            expr
        }
    }
}

/// Piecewise-linear z(t) for zoompan, built like [`crop_x_expr`] but over
/// `time` — zoompan's per-frame timestamp in seconds on the (post-cut,
/// PTS-reset) output stream. Keys always open and close at z=1.0, so the
/// image only magnifies inside each bump and rests otherwise.
pub fn zoom_z_expr(keys: &[ZoomKey]) -> String {
    match keys.len() {
        0 => "1".to_string(),
        1 => format!("{:.3}", keys[0].z),
        _ => {
            let mut expr = format!("{:.3}", keys[keys.len() - 1].z);
            for pair in keys.windows(2).rev() {
                let (a, b) = (&pair[0], &pair[1]);
                let (t0, t1) = (a.t_ms as f64 / 1000.0, b.t_ms as f64 / 1000.0);
                if t1 <= t0 {
                    continue;
                }
                expr = format!(
                    "if(lt(time\\,{t1:.3})\\,{z0:.3}+({z1:.3}-{z0:.3})*(time-{t0:.3})/{dt:.3}\\,{rest})",
                    t1 = t1,
                    z0 = a.z,
                    z1 = b.z,
                    t0 = t0,
                    dt = t1 - t0,
                    rest = expr
                );
            }
            expr
        }
    }
}

/// Split-screen graph: two full-height source columns, each a locked crop
/// centered on its face's x, stacked (left face on top) into the 9:16
/// canvas. A panel with a nonzero `dy` slides over a blurred underlay of
/// its own column, the same eye-line offset the FaceCrop path applies.
/// Panel crops keep the w:(h/2) aspect and are only ever scaled DOWN;
/// the native-window ceiling still applies.
#[allow(clippy::too_many_arguments)]
fn split_graph(
    source: &SourceInfo,
    top: crate::domain::FaceAnchor,
    bottom: crate::domain::FaceAnchor,
    w: u32,
    h: u32,
    subs: Option<&str>,
    hook_step: &str,
    vpad: &str,
    apad: &str,
) -> String {
    let subs_step = subs.map(|s| format!("{},", s)).unwrap_or_default();
    // Render space mirrors the FaceCrop path: source scaled to output
    // height when it exceeds the ceiling, native otherwise.
    let (scale_step, frame_w, frame_h) = if source.height > OUT_H {
        let scaled_w = even_round(source.width as f64 * h as f64 / source.height as f64) as u64;
        (
            format!("scale=-2:{h}:force_divisible_by=2,"),
            scaled_w,
            h as u64,
        )
    } else {
        (String::new(), source.width as u64, source.height as u64)
    };
    let crop_h = frame_h;
    let crop_w = (crop_h as f64 * (2.0 * w as f64 / h as f64)).round() as u64;
    let max_x = frame_w.saturating_sub(crop_w);
    let x_at = |cx: f32| -> u64 {
        ((cx as f64 * frame_w as f64) - crop_w as f64 / 2.0)
            .round()
            .clamp(0.0, max_x as f64) as u64
    };
    let ph = h / 2;
    // One panel's filter chain: column crop scaled to panel size; when the
    // anchor carries an eye-line offset, a blurred copy of the column fills
    // the vacated band.
    let panel = |a: &crate::domain::FaceAnchor, src_pad: &str, out_pad: &str| -> String {
        let x = x_at(a.cx);
        let base =
            format!("[{src_pad}]crop={crop_w}:{crop_h}:{x}:0,scale={w}:{ph}:force_divisible_by=2");
        if a.dy == 0.0 {
            format!("{base}[{out_pad}]")
        } else {
            format!(
                "{base},split=2[{out_pad}bg][{out_pad}fg];\
                 [{out_pad}bg]gblur=sigma=26,eq=brightness=-0.14:saturation=0.8[{out_pad}bb];\
                 [{out_pad}bb][{out_pad}fg]overlay=0:{y:.1}[{out_pad}]",
                y = a.dy as f64 * ph as f64,
            )
        }
    };
    let top_chain = panel(&top, "spa", "spt");
    let bottom_chain = panel(&bottom, "spb", "spq");
    format!(
        "[{vpad}]setpts=PTS-STARTPTS,{scale}split=2[spa][spb];\
         {top_chain};\
         {bottom_chain};\
         [spt][spq]vstack=2,scale={w}:{h}:force_divisible_by=2,{subs}{hook_step}format=yuv420p[v];\
         [{apad}]asetpts=PTS-STARTPTS[a]",
        vpad = vpad,
        apad = apad,
        scale = scale_step,
        top_chain = top_chain,
        bottom_chain = bottom_chain,
        w = w,
        h = h,
        subs = subs_step,
        hook_step = hook_step
    )
}

/// Remap clip-relative crop keys onto the post-cut output timeline, using
/// the keep list (the surviving spans of the source interval). A key inside
/// a removed gap lands on the seam where the next keep begins — crop cuts
/// only ever ride on real cut points.
fn crop_keys_to_output(keys: &[CropKey], clip_start_ms: u64, keeps: &[CutSpan]) -> Vec<CropKey> {
    let mut out: Vec<CropKey> = Vec::with_capacity(keys.len());
    for k in keys {
        let abs = clip_start_ms + k.t_ms;
        let mut t = 0u64;
        for keep in keeps {
            if keep.end_ms <= abs {
                t += keep.len_ms();
            } else if keep.start_ms <= abs {
                t += abs - keep.start_ms;
                break;
            } else {
                break;
            }
        }
        // Equal times collapse — the later face wins, matching the
        // speaker-cut intent.
        match out.last_mut() {
            Some(prev) if prev.t_ms == t => {
                prev.cx = k.cx;
                prev.dy = k.dy;
                continue;
            }
            _ => out.push(CropKey {
                t_ms: t,
                cx: k.cx,
                dy: k.dy,
            }),
        }
    }
    out
}

/// Piecewise-linear y(t) for the eye-line composite: each keyframe's `dy`
/// (a fraction of the canvas height) becomes the overlay's pixel offset.
/// Mirrors [`crop_x_expr`]: same interpolation, just vertical.
fn overlay_y_expr(keyframes: &[CropKey], canvas_h: u64) -> String {
    let ch = canvas_h as f64;
    let py = |dy: f32| -> f64 { (dy as f64 * ch).clamp(-ch, ch) };
    match keyframes.len() {
        0 => "0.0".to_string(),
        1 => format!("{:.1}", py(keyframes[0].dy)),
        _ => {
            let mut expr = format!("{:.1}", py(keyframes[keyframes.len() - 1].dy));
            for pair in keyframes.windows(2).rev() {
                let (a, b) = (&pair[0], &pair[1]);
                let (t0, t1) = (a.t_ms as f64 / 1000.0, b.t_ms as f64 / 1000.0);
                let (y0, y1) = (py(a.dy), py(b.dy));
                if t1 <= t0 {
                    continue;
                }
                expr = format!(
                    "if(lt(t\\,{t1:.3})\\,{y0:.1}+({y1:.1}-{y0:.1})*(t-{t0:.3})/{dt:.3}\\,{rest})",
                    t1 = t1,
                    y0 = y0,
                    y1 = y1,
                    t0 = t0,
                    dt = t1 - t0,
                    rest = expr
                );
            }
            expr
        }
    }
}

/// Stepped y(t) over `dy`: hard vertical cuts matching the SpeakerCrop x
/// steps, same shape as [`crop_x_expr_stepped`].
fn overlay_y_expr_stepped(keyframes: &[CropKey], canvas_h: u64) -> String {
    let ch = canvas_h as f64;
    let py = |dy: f32| -> f64 { (dy as f64 * ch).clamp(-ch, ch) };
    match keyframes.len() {
        0 => "0.0".to_string(),
        1 => format!("{:.1}", py(keyframes[0].dy)),
        _ => {
            let mut expr = format!("{:.1}", py(keyframes[keyframes.len() - 1].dy));
            for pair in keyframes.windows(2).rev() {
                let (a, b) = (&pair[0], &pair[1]);
                let t1 = b.t_ms as f64 / 1000.0;
                if t1 <= a.t_ms as f64 / 1000.0 {
                    continue;
                }
                expr = format!(
                    "if(lt(t\\,{t1:.3})\\,{y:.1}\\,{rest})",
                    y = py(a.dy),
                    rest = expr
                );
            }
            expr
        }
    }
}

/// Stepped x(t): hold each keyframe's x until the next key's time — a hard
/// cut at the boundary, no interpolation (SpeakerCrop only).
pub fn crop_x_expr_stepped(keyframes: &[CropKey], frame_w: u64, crop_w: u64) -> String {
    let max_x = frame_w.saturating_sub(crop_w) as f64;
    let px = |cx: f32| -> f64 {
        ((cx as f64) * frame_w as f64 - (crop_w as f64) / 2.0).clamp(0.0, max_x)
    };
    match keyframes.len() {
        0 => format!("{:.1}", max_x / 2.0),
        1 => format!("{:.1}", px(keyframes[0].cx)),
        _ => {
            let mut expr = format!("{:.1}", px(keyframes[keyframes.len() - 1].cx));
            for pair in keyframes.windows(2).rev() {
                let (a, b) = (&pair[0], &pair[1]);
                let t1 = b.t_ms as f64 / 1000.0;
                if t1 <= a.t_ms as f64 / 1000.0 {
                    continue;
                }
                expr = format!(
                    "if(lt(t\\,{t1:.3})\\,{x:.1}\\,{rest})",
                    x = px(a.cx),
                    rest = expr
                );
            }
            expr
        }
    }
}

/// Escape a string for use inside a single-quoted ffmpeg filter option value.
/// Backslashes are normalized to `/` (paths), `:` separates filter options,
/// and a literal `'` must close the quote, emit an escaped quote, and reopen.
fn ff_escape_str(s: &str) -> String {
    s.replace('\\', "/")
        .replace(':', "\\:")
        .replace('\'', "'\\''")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(w: u32, h: u32) -> SourceInfo {
        SourceInfo {
            filename: "s.mp4".into(),
            duration_ms: 60_000,
            width: w,
            height: h,
            fps: 30.0,
            video_codec: "h264".into(),
            audio_codec: "aac".into(),
            size_bytes: 1,
            scene_boundaries_ms: Vec::new(),
        }
    }

    #[test]
    fn single_keyframe_is_constant() {
        // 1080p native crop: window 608 wide inside the 1920-wide frame.
        // 0.5*1920 - 304 = 656
        let e = crop_x_expr(
            &[CropKey {
                t_ms: 0,
                cx: 0.5,
                dy: 0.0,
            }],
            1920,
            608,
        );
        assert_eq!(e, "656.0");
    }

    #[test]
    fn keyframes_clamp_to_frame_edges() {
        let e = crop_x_expr(
            &[CropKey {
                t_ms: 0,
                cx: 0.02,
                dy: 0.0,
            }],
            3414,
            1080,
        );
        assert_eq!(e, "0.0");
        let e = crop_x_expr(
            &[CropKey {
                t_ms: 0,
                cx: 0.99,
                dy: 0.0,
            }],
            3414,
            1080,
        );
        assert_eq!(e, format!("{:.1}", (3414 - 1080) as f64));
    }

    #[test]
    fn multi_keyframe_builds_piecewise_expression() {
        let e = crop_x_expr(
            &[
                CropKey {
                    t_ms: 0,
                    cx: 0.4,
                    dy: 0.0,
                },
                CropKey {
                    t_ms: 2000,
                    cx: 0.5,
                    dy: 0.0,
                },
                CropKey {
                    t_ms: 4000,
                    cx: 0.45,
                    dy: 0.0,
                },
            ],
            3414,
            1080,
        );
        assert!(e.starts_with("if(lt(t\\,2.000)"));
        assert!(e.contains("if(lt(t\\,4.000)"));
    }

    // ---- Downscale-only output sizing (ADR-0002) ----

    #[test]
    fn face_crop_size_is_the_native_window_capped() {
        let face = LayoutPlan::FaceCrop {
            keyframes: vec![CropKey {
                t_ms: 0,
                cx: 0.5,
                dy: 0.0,
            }],
        };
        // 1080p: window 607.5×1080 → even-rounded 608×1080 (zero resampling).
        assert_eq!(output_size(&source(1920, 1080), &face), (608, 1080));
        // 720p: 405×720 → even-rounded 406×720.
        assert_eq!(output_size(&source(1280, 720), &face), (406, 720));
        // 4K: capped at the 1080×1920 ceiling.
        assert_eq!(output_size(&source(3840, 2160), &face), (1080, 1920));
        // 360p: 202×360 — small but honest.
        assert_eq!(output_size(&source(640, 360), &face), (202, 360));
    }

    #[test]
    fn blurpad_canvas_is_the_native_916_fit() {
        // Landscape: the inscribed 9:16 box takes the full source height.
        assert_eq!(
            output_size(&source(1920, 1080), &LayoutPlan::BlurPad),
            (608, 1080)
        );
        assert_eq!(
            output_size(&source(3840, 2160), &LayoutPlan::BlurPad),
            (1080, 1920)
        );
        // Portrait: width binds instead — still a 9:16 canvas, still native.
        assert_eq!(
            output_size(&source(540, 1280), &LayoutPlan::BlurPad),
            (540, 960)
        );
    }

    #[test]
    fn output_never_exceeds_source_or_ceiling() {
        let face = LayoutPlan::FaceCrop {
            keyframes: vec![CropKey {
                t_ms: 0,
                cx: 0.5,
                dy: 0.0,
            }],
        };
        for (w, h) in [
            (1920, 1080),
            (1280, 720),
            (3840, 2160),
            (640, 360),
            (540, 1280),
            (1000, 1000),
        ] {
            for layout in [LayoutPlan::BlurPad, face.clone()] {
                let (ow, oh) = output_size(&source(w, h), &layout);
                assert!(ow <= w && oh <= h, "{w}x{h} {layout:?} → {ow}x{oh}");
                assert!(ow <= OUT_W && oh <= OUT_H, "{w}x{h} {layout:?} → {ow}x{oh}");
                assert_eq!(ow % 2, 0, "{ow}x{oh} must be even");
                assert_eq!(oh % 2, 0, "{ow}x{oh} must be even");
            }
        }
    }

    #[test]
    fn face_crop_at_native_size_crops_without_resampling() {
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::FaceCrop {
                keyframes: vec![CropKey {
                    t_ms: 0,
                    cx: 0.5,
                    dy: 0.0,
                }],
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("crop=608:1080:"), "{g}");
        assert!(!g.contains("scale"), "native window crops directly: {g}");
        // The crop window centers in the real 1920-wide frame.
        assert!(g.contains("x='656.0'"), "{g}");
    }

    #[test]
    fn face_crop_downscales_only_when_source_exceeds_the_ceiling() {
        let g = build_graph(
            &source(3840, 2160),
            &LayoutPlan::FaceCrop {
                keyframes: vec![CropKey {
                    t_ms: 0,
                    cx: 0.5,
                    dy: 0.0,
                }],
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("scale=-2:1920"), "{g}");
        assert!(g.contains("crop=1080:1920:"), "{g}");
    }

    // ---- eye-line offset ----

    #[test]
    fn centered_eye_line_keeps_the_bare_crop_graph() {
        // dy=0 renders exactly the graph used before eye-line anchoring:
        // one crop, no underlay composite.
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::FaceCrop {
                keyframes: vec![CropKey {
                    t_ms: 0,
                    cx: 0.5,
                    dy: 0.0,
                }],
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("crop=608:1080:"), "{g}");
        assert!(!g.contains("overlay"), "{g}");
    }

    #[test]
    fn nonzero_eye_line_offset_composites_over_a_blurred_underlay() {
        // dy=0.175 of the 1080-tall canvas = 189px: the crop slides down.
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::FaceCrop {
                keyframes: vec![CropKey {
                    t_ms: 0,
                    cx: 0.5,
                    dy: 0.175,
                }],
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("gblur"), "{g}");
        assert!(g.contains("crop=608:1080:x='656.0':y=0"), "{g}");
        assert!(g.contains("overlay=(W-w)/2:'189.0'"), "{g}");
    }

    #[test]
    fn eye_line_offset_downscales_only_past_the_ceiling() {
        let g = build_graph(
            &source(3840, 2160),
            &LayoutPlan::FaceCrop {
                keyframes: vec![CropKey {
                    t_ms: 0,
                    cx: 0.5,
                    dy: -0.1,
                }],
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("scale=-2:1920"), "{g}");
        assert!(g.contains("crop=1080:1920:x="), "{g}");
        // -0.1 of the 1920-tall output canvas.
        assert!(g.contains("overlay=(W-w)/2:'-192.0'"), "{g}");
    }

    #[test]
    fn speaker_crop_offsets_step_at_each_keyframe() {
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::SpeakerCrop {
                keyframes: vec![
                    CropKey {
                        t_ms: 0,
                        cx: 0.3,
                        dy: 0.1,
                    },
                    CropKey {
                        t_ms: 4_000,
                        cx: 0.7,
                        dy: -0.05,
                    },
                ],
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        // y steps between the two offsets: 108px, then -54px of the 1080
        // canvas; a hard cut like the x steps, never interpolated.
        assert!(
            g.contains("overlay=(W-w)/2:'if(lt(t\\,4.000)\\,108.0\\,-54.0)'"),
            "{g}"
        );
        assert!(!g.contains("*(t-"), "{g}");
    }

    #[test]
    fn eye_line_offsets_survive_the_auto_cut_remap() {
        let keeps = vec![keep(10_000, 20_000), keep(24_000, 40_000)];
        let keys = vec![
            CropKey {
                t_ms: 0,
                cx: 0.3,
                dy: 0.1,
            },
            CropKey {
                t_ms: 15_000,
                cx: 0.7,
                dy: -0.2,
            },
        ];
        let out = crop_keys_to_output(&keys, 10_000, &keeps);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].dy, 0.1);
        assert_eq!(out[1].t_ms, 11_000);
        assert_eq!(out[1].dy, -0.2);
        // Keys collapsing onto the same seam take the later face's dy too.
        let keys2 = vec![
            CropKey {
                t_ms: 0,
                cx: 0.3,
                dy: 0.1,
            },
            CropKey {
                t_ms: 11_000,
                cx: 0.5,
                dy: 0.05,
            },
            CropKey {
                t_ms: 12_000,
                cx: 0.7,
                dy: -0.2,
            },
        ];
        let out2 = crop_keys_to_output(&keys2, 10_000, &keeps);
        assert_eq!(out2.len(), 2);
        assert_eq!(out2[1].dy, -0.2);
    }

    #[test]
    fn layouts_without_eye_line_offsets_still_deserialize() {
        // Manifests written before eye-line framing carry no `dy` key.
        let plan: LayoutPlan =
            serde_json::from_str(r#"{"mode":"face_crop","keyframes":[{"t_ms":0,"cx":0.5}]}"#)
                .unwrap();
        assert_eq!(
            plan,
            LayoutPlan::FaceCrop {
                keyframes: vec![CropKey {
                    t_ms: 0,
                    cx: 0.5,
                    dy: 0.0,
                }],
            }
        );
    }

    #[test]
    fn split_panels_slide_on_their_own_eye_line_offsets() {
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::Split {
                top: crate::domain::FaceAnchor {
                    cx: 0.3,
                    cy: 0.3,
                    dy: 0.2,
                },
                bottom: crate::domain::FaceAnchor {
                    cx: 0.7,
                    cy: 0.7,
                    dy: -0.1,
                },
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        // Panels are 608x540; offsets land as +108px top, -54px bottom.
        assert!(g.contains("[sptbb][sptfg]overlay=0:108.0[spt]"), "top: {g}");
        assert!(
            g.contains("[spqbb][spqfg]overlay=0:-54.0[spq]"),
            "bottom: {g}"
        );
    }

    #[test]
    fn blurpad_graph_uses_the_native_scale_canvas() {
        let g = build_graph(
            &source(640, 360),
            &LayoutPlan::BlurPad,
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("scale=202:360"), "{g}");
    }

    #[test]
    fn encode_args_are_libx264_crf17_preset_fast() {
        let args = video_encode_args();
        let s = args.join(" ");
        assert!(s.contains("libx264"), "args: {s}");
        assert!(s.contains("-crf 17"), "args: {s}");
        assert!(s.contains("-preset fast"), "args: {s}");
        assert!(s.contains("-pix_fmt yuv420p"), "args: {s}");
        assert!(
            !s.to_lowercase().contains("videotoolbox"),
            "args must never select VideoToolbox: {s}"
        );
    }

    #[test]
    fn escapes_colons_in_paths() {
        assert_eq!(ff_escape_str("C:/x/y"), "C\\:/x/y");
    }

    #[test]
    fn escapes_single_quotes_for_filter_values() {
        // A quote inside a quoted value must close, escape, and reopen.
        assert_eq!(ff_escape_str("a'b"), "a'\\''b");
    }

    #[test]
    fn base_graph_has_no_subtitle_filter() {
        for layout in [
            LayoutPlan::BlurPad,
            LayoutPlan::FaceCrop {
                keyframes: vec![CropKey {
                    t_ms: 0,
                    cx: 0.5,
                    dy: 0.0,
                }],
            },
        ] {
            let g = build_graph(
                &source(1920, 1080),
                &layout,
                None,
                &[],
                10_000,
                Garnish::default(),
            );
            assert!(
                !g.contains("ass="),
                "base graph must not burn captions: {g}"
            );
            assert!(g.contains("format=yuv420p[v]"));
        }
    }

    #[test]
    fn base_graph_resets_audio_and_video_to_the_same_zero_origin() {
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("[0:v]setpts=PTS-STARTPTS"));
        assert!(g.contains("[0:a]asetpts=PTS-STARTPTS,"), "{g}");
    }

    #[test]
    fn captioned_graph_includes_subtitle_filter() {
        let subs = subtitles_filter(None, Path::new("/tmp/c.ass"));
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            Some(&subs),
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("ass='/tmp/c.ass'"));
    }

    // ---- Auto-cut concat graph ----

    fn keep(start_ms: u64, end_ms: u64) -> CutSpan {
        CutSpan { start_ms, end_ms }
    }

    #[test]
    fn cut_graph_trims_each_keep_and_concats() {
        let g = build_cut_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            CutSpec {
                keeps: &[keep(0, 4_500), keep(6_000, 16_000)],
                origin_ms: 0,
            },
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("[0:v]split=2[cv0][cv1]"), "{g}");
        assert!(g.contains("[0:a]asplit=2[ca0][ca1]"), "{g}");
        assert!(g.contains("trim=start=0.000:duration=4.500"), "{g}");
        assert!(g.contains("trim=start=6.000:duration=10.000"), "{g}");
        assert!(g.contains("atrim=start=0.000:duration=4.500"), "{g}");
        assert!(g.contains("[cvt0][cvt1]concat=n=2:v=1:a=0[cvj]"), "{g}");
        assert!(g.contains("[cat0][cat1]concat=n=2:v=0:a=1[caj]"), "{g}");
        // The normal framing graph then shapes the joined stream.
        assert!(g.contains("[cvj]setpts=PTS-STARTPTS"), "{g}");
        assert!(g.contains("gblur"), "{g}");
        assert!(g.contains("[caj]asetpts=PTS-STARTPTS,"), "{g}");
    }

    #[test]
    fn cut_graph_trim_times_are_relative_to_the_seek_origin() {
        // Clip starting at 60 s: keep times minus 60 s = stream-relative.
        let g = build_cut_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            CutSpec {
                keeps: &[keep(60_000, 62_000), keep(65_000, 70_000)],
                origin_ms: 60_000,
            },
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("trim=start=0.000:duration=2.000"), "{g}");
        assert!(g.contains("trim=start=5.000:duration=5.000"), "{g}");
    }

    #[test]
    fn cut_graph_keeps_the_face_crop_pipeline() {
        let g = build_cut_graph(
            &source(1920, 1080),
            &LayoutPlan::FaceCrop {
                keyframes: vec![CropKey {
                    t_ms: 0,
                    cx: 0.5,
                    dy: 0.0,
                }],
            },
            None,
            CutSpec {
                keeps: &[keep(0, 4_500), keep(6_000, 16_000)],
                origin_ms: 0,
            },
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("crop=608:1080:"), "{g}");
        assert!(g.contains("concat=n=2"), "{g}");
    }

    // ---- Audio leveling ----

    #[test]
    fn audio_chain_normalizes_loudness_and_fades_edges() {
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            &[],
            30_000,
            Garnish::default(),
        );
        assert!(g.contains("loudnorm=I=-16:TP=-1.5:LRA=11"), "{g}");
        assert!(g.contains("afade=t=in:st=0:d=0.05"), "{g}");
        // Tail fade starts 80 ms before the output end.
        assert!(g.contains("afade=t=out:st=29.920:d=0.08"), "{g}");
    }

    #[test]
    fn cut_graph_fades_against_the_joined_duration() {
        // Output length is the sum of keeps (4.5 s + 10 s), not the read window.
        let g = build_cut_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            CutSpec {
                keeps: &[keep(0, 4_500), keep(6_000, 16_000)],
                origin_ms: 0,
            },
            &[],
            14_500,
            Garnish::default(),
        );
        assert!(g.contains("afade=t=out:st=14.420:d=0.08"), "{g}");
    }

    // ---- Zoom cuts ----

    fn zk(t_ms: u64, z: f32) -> ZoomKey {
        ZoomKey { t_ms, z }
    }

    #[test]
    fn zoom_keys_add_a_zoompan_step_inside_the_face_crop() {
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::FaceCrop {
                keyframes: vec![CropKey {
                    t_ms: 0,
                    cx: 0.5,
                    dy: 0.0,
                }],
            },
            None,
            &[zk(0, 1.0), zk(1_000, 1.07), zk(2_000, 1.0)],
            10_000,
            Garnish::default(),
        );
        // The zoom sits between the crop and the pixel-format fix, sizing
        // back to the output window — the crop's locked x is untouched.
        assert!(g.contains("crop=608:1080:x='656.0':y=0,zoompan="), "{g}");
        assert!(g.contains("zoompan=z='"), "{g}");
        assert!(g.contains(":s=608x1080:"), "{g}");
        // fps follows the source — the default 25 would retime the video
        // and desync it from the audio.
        assert!(g.contains(":fps=30.000,"), "{g}");
        assert!(g.contains(":d=1:"), "{g}");
    }

    #[test]
    fn zoom_expression_is_piecewise_and_rests_at_one() {
        assert_eq!(zoom_z_expr(&[]), "1");
        assert_eq!(zoom_z_expr(&[zk(0, 1.07)]), "1.070");
        let e = zoom_z_expr(&[zk(0, 1.0), zk(350, 1.07), zk(900, 1.0)]);
        // Innermost segment (350–900 ms) is the fall back to rest.
        assert!(e.contains("if(lt(time\\,0.900)"), "{e}");
        assert!(e.contains("1.070+(1.000-1.070)*(time-0.350)/0.550"), "{e}");
        // Everything after the last key rests at z=1.
        assert!(e.ends_with("1.000))"), "{e}");
    }

    #[test]
    fn zoom_is_skipped_for_blurpad_and_empty_key_lists() {
        // BlurPad composites its own canvas — there is no Locked crop to
        // punch inside, so zoom keys never reach the graph.
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            &[zk(0, 1.0), zk(1_000, 1.07), zk(2_000, 1.0)],
            10_000,
            Garnish::default(),
        );
        assert!(!g.contains("zoompan"), "{g}");
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::FaceCrop {
                keyframes: vec![CropKey {
                    t_ms: 0,
                    cx: 0.5,
                    dy: 0.0,
                }],
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(!g.contains("zoompan"), "{g}");
        // An unparseable source fps would retime the output — skip instead.
        let mut no_fps = source(1920, 1080);
        no_fps.fps = 0.0;
        let g = build_graph(
            &no_fps,
            &LayoutPlan::FaceCrop {
                keyframes: vec![CropKey {
                    t_ms: 0,
                    cx: 0.5,
                    dy: 0.0,
                }],
            },
            None,
            &[zk(0, 1.0), zk(1_000, 1.07), zk(2_000, 1.0)],
            10_000,
            Garnish::default(),
        );
        assert!(!g.contains("zoompan"), "{g}");
    }

    #[test]
    fn subtitles_filter_escapes_fonts_dir() {
        let s = subtitles_filter(Some(Path::new("/a'b")), Path::new("/tmp/c.ass"));
        assert!(s.contains("fontsdir='/a'\\''b'"));
    }

    #[test]
    fn speaker_crop_steps_instead_of_interpolating() {
        // Two cuts: x holds each face's position until the next key time —
        // no ramp term anywhere in the expression.
        let e = crop_x_expr_stepped(
            &[
                CropKey {
                    t_ms: 0,
                    cx: 0.3,
                    dy: 0.0,
                },
                CropKey {
                    t_ms: 5_000,
                    cx: 0.7,
                    dy: 0.0,
                },
            ],
            1920,
            608,
        );
        // x(0.3) = 272, x(0.7) = 1040 — stepped: hold 272 until t=5s, then 1040.
        assert_eq!(e, "if(lt(t\\,5.000)\\,272.0\\,1040.0)", "{e}");
        assert!(!e.contains("(t-"), "stepped expr must not interpolate: {e}");
    }

    #[test]
    fn crop_keys_retime_onto_the_output_timeline() {
        // Clip 10–40 s; a 4 s removal at 20–24 s. A speaker cut at source
        // 25 s (clip-relative 15 s) lands at 11 s output.
        let keeps = vec![
            CutSpan {
                start_ms: 10_000,
                end_ms: 20_000,
            },
            CutSpan {
                start_ms: 24_000,
                end_ms: 40_000,
            },
        ];
        let keys = vec![
            CropKey {
                t_ms: 0,
                cx: 0.3,
                dy: 0.0,
            },
            CropKey {
                t_ms: 15_000,
                cx: 0.7,
                dy: 0.0,
            },
        ];
        let out = crop_keys_to_output(&keys, 10_000, &keeps);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].t_ms, 0);
        assert_eq!(out[1].t_ms, 11_000);
    }

    #[test]
    fn crop_key_inside_a_removal_lands_on_the_seam() {
        // Speaker cut at source 21 s sits inside the 20–24 s removal → it
        // rides onto the seam (10 s output), right where the auto-cut seam
        // already switches content.
        let keeps = vec![
            CutSpan {
                start_ms: 10_000,
                end_ms: 20_000,
            },
            CutSpan {
                start_ms: 24_000,
                end_ms: 40_000,
            },
        ];
        let keys = vec![
            CropKey {
                t_ms: 0,
                cx: 0.3,
                dy: 0.0,
            },
            CropKey {
                t_ms: 11_000,
                cx: 0.7,
                dy: 0.0,
            },
        ];
        let out = crop_keys_to_output(&keys, 10_000, &keeps);
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out[1].t_ms, 10_000);
        assert!((out[1].cx - 0.7).abs() < 1e-6);
        // Two keys inside the same removal collapse onto the seam — the
        // later face wins, matching the speaker-cut intent.
        let keys2 = vec![
            CropKey {
                t_ms: 0,
                cx: 0.3,
                dy: 0.0,
            },
            CropKey {
                t_ms: 11_000,
                cx: 0.5,
                dy: 0.0,
            },
            CropKey {
                t_ms: 12_000,
                cx: 0.7,
                dy: 0.0,
            },
        ];
        let out2 = crop_keys_to_output(&keys2, 10_000, &keeps);
        assert_eq!(out2.len(), 2, "{out2:?}");
        assert!((out2[1].cx - 0.7).abs() < 1e-6);
    }

    #[test]
    fn split_graph_stacks_two_face_columns() {
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::Split {
                top: crate::domain::FaceAnchor {
                    cx: 0.3,
                    cy: 0.45,
                    dy: 0.0,
                },
                bottom: crate::domain::FaceAnchor {
                    cx: 0.7,
                    cy: 0.45,
                    dy: 0.0,
                },
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        // Panel crop = 1080 tall × 1216 wide (9:8) around each face's x.
        assert!(g.contains("vstack=2"), "{g}");
        assert!(g.contains("crop=1216:1080:0:0"), "left face column: {g}");
        assert!(g.contains("crop=1216:1080:704:0"), "right face column: {g}");
        assert!(g.contains("scale=608:540"), "{g}");
    }

    #[test]
    fn speaker_crop_uses_the_stepped_expression() {
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::SpeakerCrop {
                keyframes: vec![
                    CropKey {
                        t_ms: 0,
                        cx: 0.3,
                        dy: 0.0,
                    },
                    CropKey {
                        t_ms: 4_000,
                        cx: 0.7,
                        dy: 0.0,
                    },
                ],
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("if(lt(t\\,4.000)\\,272.0\\,1040.0)"), "{g}");
        assert!(!g.contains("*(t-"), "{g}");
    }

    #[test]
    fn portrait_source_falls_back_to_blur_pad() {
        // 540×1280 is narrower than 9:16 — the crop window cannot fit.
        let g = build_graph(
            &source(540, 1280),
            &LayoutPlan::FaceCrop {
                keyframes: vec![CropKey {
                    t_ms: 0,
                    cx: 0.5,
                    dy: 0.0,
                }],
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(
            g.contains("gblur"),
            "portrait must fall back to blur-pad: {g}"
        );
    }

    // ---- End card ----

    /// The bundled face the card is typeset in (relative to the crate root —
    /// tests run with it as cwd).
    fn card_font() -> PathBuf {
        PathBuf::from("assets/fonts/Inter-ExtraBold.ttf")
    }

    #[test]
    fn end_card_appends_a_generated_tail_after_the_clip() {
        let font = card_font();
        assert!(font.is_file(), "bundled display font missing");
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            &[],
            10_000,
            Garnish {
                card: Some(font.as_path()),
                ..Default::default()
            },
        );
        // The framing body emits intermediate pads the card concat consumes.
        assert!(g.contains("format=yuv420p[v0]"), "{g}");
        assert!(g.contains("[a0]"), "{g}");
        // The tail: sized/fps-matched card, drawn line, fade-in, silent pad.
        let (w, h) = output_size(&source(1920, 1080), &LayoutPlan::BlurPad);
        assert!(
            g.contains(&format!("color=c=0x0B0B0F:s={w}x{h}:r=30.000:d=1.200")),
            "{g}"
        );
        assert!(g.contains("text='Made with'"), "{g}");
        assert!(g.contains("text='Clipping Factory'"), "{g}");
        assert!(g.contains("fade=t=in:st=0:d=0.15"), "{g}");
        assert!(g.contains("anullsrc=r=48000:cl=stereo:d=1.200"), "{g}");
        assert!(g.contains("[v0][cardv]concat=n=2:v=1:a=0[v]"), "{g}");
        assert!(g.contains("[a0][carda]concat=n=2:v=0:a=1[a]"), "{g}");
        // The audio fade still keys off the clip duration, not clip + card.
        assert!(g.contains("afade=t=out:st=9.920:d=0.08"), "{g}");
    }

    #[test]
    fn end_card_off_emits_the_final_pads_directly() {
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("format=yuv420p[v]"), "{g}");
        assert!(!g.contains("v0]"), "{g}");
        assert!(!g.contains("color=c=0x0B0B0F"), "{g}");
    }

    #[test]
    fn cut_graph_places_the_card_after_the_joined_clip() {
        let g = build_cut_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            CutSpec {
                keeps: &[keep(0, 4_500), keep(6_000, 16_000)],
                origin_ms: 0,
            },
            &[],
            14_500,
            Garnish {
                card: Some(card_font().as_path()),
                ..Default::default()
            },
        );
        // Card tail comes last, after the keep-concat and framing body.
        let card_at = g.find("color=c=0x0B0B0F").expect("card missing");
        let join_at = g.find("[caj]").expect("join missing");
        assert!(card_at > join_at, "card must follow the joined clip: {g}");
        assert!(g.contains("concat=n=2:v=1:a=0[v]"), "{g}");
    }

    #[test]
    fn end_card_lockup_fits_the_frame_width() {
        // The big line is ~16 ExtraBold glyphs; sized at 8.2% of the output
        // width it stays inside even the narrowest 406px render.
        for (sw, sh) in [(1280, 720), (1920, 1080)] {
            let g = build_graph(
                &source(sw, sh),
                &LayoutPlan::BlurPad,
                None,
                &[],
                10_000,
                Garnish {
                    card: Some(card_font().as_path()),
                    ..Default::default()
                },
            );
            let (w, _h) = output_size(&source(sw, sh), &LayoutPlan::BlurPad);
            let want = format!("fontsize={:.0}", w as f64 * 0.082);
            assert!(g.contains(&want), "{w}px-wide output wants {want}: {g}");
        }
    }

    #[test]
    fn end_card_ms_counts_only_when_the_font_resolves() {
        let mut cfg = Config::resolve();
        cfg.fonts_dir = Some(PathBuf::from("assets/fonts"));
        assert_eq!(end_card_ms(&cfg, true), END_CARD_MS);
        assert_eq!(end_card_ms(&cfg, false), 0);
        cfg.fonts_dir = Some(PathBuf::from("/nonexistent"));
        assert_eq!(end_card_ms(&cfg, true), 0);
    }

    // ---- Progress bar garnish ----

    #[test]
    fn progress_bar_draws_a_drawbox_filling_over_the_clip_duration() {
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            &[],
            10_000,
            Garnish {
                bar: Some("#ffaa00"),
                ..Default::default()
            },
        );
        assert!(
            g.contains("drawbox=x=0:y='ih-6':w='trunc(iw*t/10.000)':h=6:color=0xffaa00@0.9:t=fill,format=yuv420p"),
            "{g}"
        );
    }

    #[test]
    fn progress_bar_sanitizes_the_accent_and_stays_off_by_default() {
        let plain = build_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(!plain.contains("drawbox"), "{plain}");
        let dirty = progress_bar_step("0xff<script>", 5_000);
        assert!(!dirty.contains('<'), "{dirty}");
        assert!(dirty.contains("color=0x0ffc"), "{dirty}");
    }

    #[test]
    fn progress_bar_reaches_the_concat_graph_too() {
        let g = build_cut_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            CutSpec {
                keeps: &[keep(0, 4_500), keep(6_000, 16_000)],
                origin_ms: 0,
            },
            &[],
            14_500,
            Garnish {
                bar: Some("#ffffff"),
                ..Default::default()
            },
        );
        assert!(g.contains("drawbox"), "{g}");
    }

    // ---- Hook title garnish ----

    fn hook(lines: &[String]) -> Option<HookTitle<'_>> {
        Some(HookTitle {
            lines,
            fontfile: None,
            font: "Inter",
        })
    }

    #[test]
    fn hook_title_draws_a_timed_title_card_in_the_upper_band() {
        let lines = wrap_hook_title("the quick brown fox jumps over the lazy dog", true);
        assert_eq!(lines, vec!["THE QUICK BROWN FOX", "JUMPS OVER THE LAZY…"]);
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            &[],
            10_000,
            Garnish {
                hook: hook(&lines),
                ..Default::default()
            },
        );
        // min(1.8, 25% of 10 s) — the card drops after the opening beat.
        assert_eq!(g.matches("enable='between(t,0,1.80)'").count(), 2, "{g}");
        assert!(g.contains("text='THE QUICK BROWN FOX'"), "{g}");
        assert!(g.contains("text='JUMPS OVER THE LAZY…'"), "{g}");
        // Upper band: ~14% of the 1080-tall output, clear of the caption zone.
        assert!(g.contains("y=151"), "{g}");
        // Font falls back to the family name when no bundled file resolves.
        assert!(g.contains("font='Inter'"), "{g}");
        assert!(g.contains("borderw="), "{g}");
    }

    #[test]
    fn hook_title_window_shrinks_below_a_quarter_of_the_clip() {
        let lines = wrap_hook_title("hi", true);
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            &[],
            4_000,
            Garnish {
                hook: hook(&lines),
                ..Default::default()
            },
        );
        assert!(g.contains("enable='between(t,0,1.00)'"), "{g}");
    }

    #[test]
    fn hook_title_is_absent_by_default_and_noops_on_empty_text() {
        let plain = build_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(!plain.contains("drawtext"), "{plain}");

        let empty = build_graph(
            &source(1920, 1080),
            &LayoutPlan::BlurPad,
            None,
            &[],
            10_000,
            Garnish {
                hook: hook(&[]),
                ..Default::default()
            },
        );
        assert!(!empty.contains("drawtext"), "{empty}");
    }

    #[test]
    fn hook_title_wrap_respects_the_char_budget() {
        // Word wrap: 19 chars + " JUMPS" would overflow, so it wraps.
        assert_eq!(
            wrap_hook_title("the quick brown fox jumps", false),
            vec!["the quick brown fox", "jumps"]
        );
        // A single over-long word hard-splits inside the budget.
        let long = wrap_hook_title("supercalifragilisticexpialidocious", true);
        assert_eq!(long, vec!["SUPERCALIFRAGILISTICEX", "PIALIDOCIOUS"]);
        for l in &long {
            assert!(l.chars().count() <= HOOK_LINE_CHARS, "{l}");
        }
        // Blank input wraps to nothing.
        assert!(wrap_hook_title("   ", true).is_empty());
    }

    #[test]
    fn hook_title_text_escapes_filter_and_drawtext_metachars() {
        assert_eq!(drawtext_escape("a:b,c%d'e\\f\nz"), "a\\:b\\,c%%d'\\''e/f z");
    }

    #[test]
    fn hook_title_uses_the_heaviest_bundled_face() {
        let mut cfg = Config::resolve();
        cfg.fonts_dir = Some(PathBuf::from("assets/fonts"));
        assert_eq!(
            hook_font_file(&cfg, "Inter"),
            Some(PathBuf::from("assets/fonts/Inter-ExtraBold.ttf"))
        );
        assert_eq!(
            hook_font_file(&cfg, "Anton"),
            Some(PathBuf::from("assets/fonts/Anton-Regular.ttf"))
        );
        assert_eq!(hook_font_file(&cfg, "Georgia"), None);
        cfg.fonts_dir = Some(PathBuf::from("/nonexistent"));
        assert_eq!(hook_font_file(&cfg, "Inter"), None);
    }
}
