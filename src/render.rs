//! Clip rendering (PRD §11–12), in two passes:
//!
//! 1. [`render_base_clip`] — one continuous source interval → framed
//!    vertical H.264/AAC MP4 **without captions**, sized by [`output_size`]
//!    (native crop window, capped at 1080×1920 — ADR-0002). This is the
//!    expensive pass (decode, crop or blur per shot, encode). The
//!    base is kept on disk so caption styling can change later without
//!    re-doing it.
//! 2. [`burn_captions`] — base MP4 + generated ASS → final captioned MP4.
//!    Fast: the video is re-encoded at output size with only the subtitle
//!    filter, and the audio stream is copied bit-for-bit.
//!
//! Two layouts (house style, §11.2/11.3):
//! - BlurPad:  source centered over a blurred, darkened copy of itself.
//! - FaceCrop: one static view per shot (ADR-0004) — a crop on the person
//!   speaking, or the BlurPad treatment for a shot with nobody in it —
//!   joined with hard cuts. Nothing ever pans.

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
/// - FaceCrop: `source_h × 9/16` wide × `source_h` tall, capped.
/// - BlurPad: the largest 9:16 canvas inscribed in the source, capped.
pub fn output_size(source: &SourceInfo, layout: &LayoutPlan) -> (u32, u32) {
    let (sw, sh) = (source.width as f64, source.height as f64);
    match layout {
        LayoutPlan::FaceCrop { .. } if face_window_fits(source) => {
            let h = source.height.min(OUT_H) & !1;
            let w = even_round(h as f64 * 9.0 / 16.0).min(OUT_W);
            (w, h)
        }
        // BlurPad, or a FaceCrop window that cannot fit the source.
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
/// over the framed canvas, applied only by the FaceCrop path.
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
    bar: Option<&str>,
    hook: Option<HookSpec<'_>>,
    out_path: &Path,
    cancel: &CancellationToken,
    mut on_progress: F,
) -> Result<()>
where
    F: FnMut(f32),
{
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
    let out_dur_s = clip_dur_ms as f64 / 1000.0;
    let layout = &retime_layout(layout, start_ms, keeps);
    // The hook title's wrapped lines and resolved face are owned here and
    // borrowed by the garnish for the graph build below.
    let hook_lines = hook
        .map(|h| wrap_hook_title(h.headline, h.caps))
        .unwrap_or_default();
    let hook_file = hook.and_then(|h| hook_font_file(cfg, h.font));
    let garnish = Garnish {
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
/// (`out_*`).
struct Pads<'a> {
    in_v: &'a str,
    in_a: &'a str,
    out_v: &'a str,
    out_a: &'a str,
}

/// Opt-in garnish applied to a base render: the progress bar (accent hex
/// or None) and the hook title (opening title card, None unless the toggle is on
/// and the headline wrapped to at least one line).
#[derive(Clone, Copy, Default)]
struct Garnish<'a> {
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
    graph_body(source, layout, subs, zoom, dur_ms, ("0:v", "0:a"), garnish)
}

/// The framing body, reading from the given input pads.
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
    let pads = Pads {
        in_v,
        in_a,
        out_v: "v",
        out_a: "a",
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
        LayoutPlan::FaceCrop { keyframes } if face_window_fits(source) => Some(keyframes),
        _ => None,
    };
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
            // Zoom cuts: a punch inside the framed canvas (the views never
            // move). zoompan re-scales a centered subregion back to output
            // size; between keys the expression is exactly 1.0, i.e.
            // pixel-identical to no zoom. fps must follow the source —
            // zoompan defaults to 25 and would otherwise retime the video
            // out of sync with the audio.
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
            let views = shot_views(keyframes, dur_ms);
            let mut g = format!("[{vpad}]setpts=PTS-STARTPTS");
            if views.len() == 1 {
                g.push_str("[fin];");
                g.push_str(&view_graph(source, views[0].2, (w, h), "fin", "framed"));
            } else {
                // One trimmed branch per view, each framed statically, then
                // joined: the view changes are hard cuts on exact frames.
                g.push_str(&format!(",split={}", views.len()));
                for i in 0..views.len() {
                    g.push_str(&format!("[fin{i}]"));
                }
                g.push(';');
                for (i, (start, end, key)) in views.iter().enumerate() {
                    let end = end.map(|e| format!(":end={e:.3}")).unwrap_or_default();
                    g.push_str(&format!(
                        "[fin{i}]trim=start={start:.3}{end},setpts=PTS-STARTPTS[fcut{i}];"
                    ));
                    g.push_str(&view_graph(
                        source,
                        key,
                        (w, h),
                        &format!("fcut{i}"),
                        &format!("fv{i}"),
                    ));
                    g.push(';');
                }
                for i in 0..views.len() {
                    g.push_str(&format!("[fv{i}]"));
                }
                g.push_str(&format!("concat=n={}:v=1:a=0[framed]", views.len()));
            }
            g.push_str(&format!(
                ";[framed]{zoom_step}{subs_step}{hook_step}{bar_step}format=yuv420p[{vout}];\
                 [{apad}]{audio}[{aout}]"
            ));
            g
        }
    }
}

/// The views of a FaceCrop plan as (start s, end s, key) on the output
/// timeline; the last view runs to the end. Views that start past the end
/// or last under a frame are dropped, and the first always starts at 0.
fn shot_views(keyframes: &[CropKey], dur_ms: u64) -> Vec<(f64, Option<f64>, &CropKey)> {
    let mut keys: Vec<&CropKey> = Vec::new();
    for k in keyframes {
        if k.t_ms + MIN_VIEW_MS > dur_ms && !keys.is_empty() {
            break;
        }
        match keys.last() {
            Some(prev) if k.t_ms < prev.t_ms + MIN_VIEW_MS => {
                *keys.last_mut().expect("non-empty") = k;
            }
            _ => keys.push(k),
        }
    }
    if keys.is_empty() {
        return vec![(0.0, None, &DEFAULT_VIEW)];
    }
    (0..keys.len())
        .map(|i| {
            let at = |k: &CropKey| k.t_ms.saturating_sub(CUT_SLACK_MS) as f64 / 1000.0;
            let start = if i == 0 { 0.0 } else { at(keys[i]) };
            let end = keys.get(i + 1).map(|&n| at(n));
            (start, end, keys[i])
        })
        .collect()
}

/// A view shorter than this (about one frame) is dropped.
const MIN_VIEW_MS: u64 = 40;
/// View boundaries land this much early: a cut's time is rounded to the
/// ms, and rounding past the first frame of the new shot would show that
/// frame with the previous view. Frames are ≥16 ms apart, so this never
/// pulls in a frame from before the cut.
const CUT_SLACK_MS: u64 = 5;

const DEFAULT_VIEW: CropKey = CropKey {
    t_ms: 0,
    cx: 0.5,
    cy: 0.5,
    zoom: 1.0,
    pad: false,
};

/// Frame one view from `[input]` to `[output]` at the `w`×`h` canvas: a
/// static crop of the source, resized only when the window differs from
/// the canvas, or — for a pad view — the full frame over a blurred copy.
fn view_graph(
    source: &SourceInfo,
    key: &CropKey,
    (w, h): (u32, u32),
    input: &str,
    output: &str,
) -> String {
    if key.pad {
        return format!(
            "[{input}]split=2[{output}b][{output}f];\
             [{output}b]scale={w}:{h}:force_original_aspect_ratio=increase:force_divisible_by=2,\
             crop={w}:{h},gblur=sigma=26,eq=brightness=-0.14:saturation=0.8[{output}bg];\
             [{output}f]scale={w}:{h}:force_original_aspect_ratio=decrease:force_divisible_by=2[{output}fg];\
             [{output}bg][{output}fg]overlay=(W-w)/2:(H-h)/2,setsar=1[{output}]"
        );
    }
    let (cw, ch, x, y) = crop_window(source, key);
    let scale = if (cw, ch) == (w, h) {
        String::new()
    } else {
        format!(",scale={w}:{h}:flags=lanczos")
    };
    format!("[{input}]crop={cw}:{ch}:{x}:{y}{scale},setsar=1[{output}]")
}

/// A view's crop window in source pixels: 9:16, `source_h / zoom` tall,
/// centered on the key and clamped inside the frame.
fn crop_window(source: &SourceInfo, key: &CropKey) -> (u32, u32, u32, u32) {
    let (sw, sh) = (source.width, source.height);
    let ch = even_floor(sh as f64 / (key.zoom as f64).max(1.0)).max(2);
    let cw = even_round(ch as f64 * 9.0 / 16.0).min(sw & !1);
    let place = |c: f32, full: u32, win: u32| -> u32 {
        let max = full.saturating_sub(win) as f64;
        (c as f64 * full as f64 - win as f64 / 2.0)
            .clamp(0.0, max)
            .round() as u32
    };
    (cw, ch, place(key.cx, sw, cw), place(key.cy, sh, ch))
}

/// Move a plan's view times from the clip's source timeline onto the
/// rendered one: auto-cut removes time, and a single kept span starts
/// the input late. A view that starts inside removed time starts where
/// the next kept span does.
fn retime_layout(layout: &LayoutPlan, clip_start_ms: u64, keeps: &[CutSpan]) -> LayoutPlan {
    let LayoutPlan::FaceCrop { keyframes } = layout else {
        return layout.clone();
    };
    if keeps.is_empty() {
        return layout.clone();
    }
    let out = |t_rel: u64| -> u64 {
        let t = clip_start_ms + t_rel;
        keeps
            .iter()
            .map(|k| t.clamp(k.start_ms, k.end_ms) - k.start_ms)
            .sum()
    };
    LayoutPlan::FaceCrop {
        keyframes: keyframes
            .iter()
            .map(|k| CropKey {
                t_ms: out(k.t_ms),
                ..k.clone()
            })
            .collect(),
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
    g
}

/// Opt-in progress bar: a thin accent-colored strip along the bottom edge
/// filling left-to-right over the clip's content duration. `hex` is an "#RRGGBB" accent; only hex digits survive into the
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

/// Piecewise-linear z(t) for zoompan, over `time` — zoompan's per-frame timestamp in seconds on the (post-cut,
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

    // ---- Downscale-only output sizing (ADR-0002) ----

    #[test]
    fn face_crop_size_is_the_native_window_capped() {
        let face = LayoutPlan::FaceCrop {
            keyframes: vec![CropKey::crop(0, 0.5, 0.5, 1.0)],
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
            keyframes: vec![CropKey::crop(0, 0.5, 0.5, 1.0)],
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
                keyframes: vec![CropKey::crop(0, 0.5, 0.5, 1.0)],
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        // The crop window centers in the real 1920-wide frame.
        assert!(g.contains("crop=608:1080:656:0,setsar=1"), "{g}");
        assert!(!g.contains("scale"), "native window crops directly: {g}");
    }

    #[test]
    fn face_crop_downscales_only_when_source_exceeds_the_ceiling() {
        let g = build_graph(
            &source(3840, 2160),
            &LayoutPlan::FaceCrop {
                keyframes: vec![CropKey::crop(0, 0.5, 0.5, 1.0)],
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("crop=1216:2160:1312:0,scale=1080:1920"), "{g}");
    }

    #[test]
    fn a_zoomed_view_crops_tighter_and_scales_to_the_canvas() {
        // zoom 1.5 on 1080p: a 720-tall window placed by cx/cy, sized back
        // up to the 608×1080 canvas.
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::FaceCrop {
                keyframes: vec![CropKey::crop(0, 0.5, 0.4, 1.5)],
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(
            g.contains("crop=406:720:757:72,scale=608:1080:flags=lanczos"),
            "{g}"
        );
    }

    #[test]
    fn views_hard_cut_on_their_start_times_and_never_pan() {
        let g = build_graph(
            &source(1920, 1080),
            &LayoutPlan::FaceCrop {
                keyframes: vec![
                    CropKey::crop(0, 0.3, 0.5, 1.0),
                    CropKey::pad(2_500),
                    CropKey::crop(6_000, 0.7, 0.5, 1.0),
                ],
            },
            None,
            &[],
            10_000,
            Garnish::default(),
        );
        assert!(g.contains("split=3[fin0][fin1][fin2]"), "{g}");
        assert!(g.contains("[fin0]trim=start=0.000:end=2.495,"), "{g}");
        assert!(g.contains("[fin1]trim=start=2.495:end=5.995,"), "{g}");
        assert!(g.contains("[fin2]trim=start=5.995,"), "{g}");
        // Each view is a constant crop; the pad view blurs the full frame.
        assert!(
            g.contains("[fcut0]crop=608:1080:272:0,setsar=1[fv0]"),
            "{g}"
        );
        assert!(
            g.contains("[fcut2]crop=608:1080:1040:0,setsar=1[fv2]"),
            "{g}"
        );
        assert!(g.contains("[fv1bg][fv1fg]overlay"), "{g}");
        assert!(
            g.contains("[fv0][fv1][fv2]concat=n=3:v=1:a=0[framed]"),
            "{g}"
        );
        assert!(!g.contains("if(lt(t"), "no time-varying crop: {g}");
    }

    #[test]
    fn views_past_the_end_or_under_a_frame_are_dropped() {
        let keys = [
            CropKey::crop(0, 0.3, 0.5, 1.0),
            CropKey::crop(10, 0.4, 0.5, 1.0),
            CropKey::crop(5_000, 0.6, 0.5, 1.0),
            CropKey::crop(12_000, 0.7, 0.5, 1.0),
        ];
        let views = shot_views(&keys, 10_000);
        assert_eq!(views.len(), 2, "{views:?}");
        assert_eq!(views[0].0, 0.0);
        assert_eq!(views[0].1, Some(4.995));
        assert_eq!(views[0].2.cx, 0.4);
        assert_eq!(views[1].1, None);
    }

    #[test]
    fn auto_cut_moves_views_onto_the_rendered_timeline() {
        let layout = LayoutPlan::FaceCrop {
            keyframes: vec![
                CropKey::crop(0, 0.3, 0.5, 1.0),
                CropKey::crop(5_000, 0.7, 0.5, 1.0),
                CropKey::crop(8_000, 0.5, 0.5, 1.0),
            ],
        };
        // Clip at 60 s; 2 s removed at 62–64 s, so 65 s renders at 3 s.
        let keeps = [keep(60_000, 62_000), keep(64_000, 70_000)];
        let LayoutPlan::FaceCrop { keyframes } = retime_layout(&layout, 60_000, &keeps) else {
            panic!("layout kind changed");
        };
        let times: Vec<u64> = keyframes.iter().map(|k| k.t_ms).collect();
        assert_eq!(times, vec![0, 3_000, 6_000]);
        // One kept span that starts late shifts every view with it.
        let LayoutPlan::FaceCrop { keyframes } =
            retime_layout(&layout, 60_000, &[keep(61_000, 70_000)])
        else {
            panic!("layout kind changed");
        };
        let times: Vec<u64> = keyframes.iter().map(|k| k.t_ms).collect();
        assert_eq!(times, vec![0, 4_000, 7_000]);
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
                keyframes: vec![CropKey::crop(0, 0.5, 0.5, 1.0)],
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
                keyframes: vec![CropKey::crop(0, 0.5, 0.5, 1.0)],
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
                keyframes: vec![CropKey::crop(0, 0.5, 0.5, 1.0)],
            },
            None,
            &[zk(0, 1.0), zk(1_000, 1.07), zk(2_000, 1.0)],
            10_000,
            Garnish::default(),
        );
        // The zoom sits between the crop and the pixel-format fix, sizing
        // back to the output window — the crop itself is untouched.
        assert!(g.contains("[framed]zoompan="), "{g}");
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
        // A whole-clip BlurPad has no face to punch in on, so zoom keys
        // never reach the graph.
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
                keyframes: vec![CropKey::crop(0, 0.5, 0.5, 1.0)],
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
                keyframes: vec![CropKey::crop(0, 0.5, 0.5, 1.0)],
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
    fn portrait_source_falls_back_to_blur_pad() {
        // 540×1280 is narrower than 9:16 — the crop window cannot fit.
        let g = build_graph(
            &source(540, 1280),
            &LayoutPlan::FaceCrop {
                keyframes: vec![CropKey::crop(0, 0.5, 0.5, 1.0)],
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
