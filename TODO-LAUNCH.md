# To-Do: Make Money From Clipping Factory

**Direction (2026-08-12): FREE-FIRST pivot.** The app is MIT open source — free is the funnel, not the enemy. The paid tier is not a date on the calendar; it's a **trigger**: the moment the free hosted server backs up, that's demand proof → then charge for priority/volume. Until then: free hosted pilot + streaming + OSS credits. CF serves the channel and ecom, it is not a second job.

## 1. Free hosted pilot (spare M1 MacBook at home = "the cloud")

- [ ] Tailscale on spare MacBook + main machine (wiki already parked this pattern: GBrain = spare MacBook + Tailscale)
- [ ] **Tailscale Funnel** → public HTTPS URL, no port forwarding, no VPS (this is "the fast way")
- [ ] Auth in front of the studio (app has NO auth — localhost design; anyone with the URL gets the MacBook). Basic token/cookie in front of Funnel.
- [ ] launchd/KeepAlive agent on the spare (copy the pattern from the live server, `com.clipping-factory.server`)
- [ ] Battery: set built-in **80% Charge Limit** on the spare (System Settings → Battery → Battery Health) — plug in forever, battery parks at 80%, no degradation. Original Apple brick only, hard surface, vents clear (fire safety = that's the whole list)
- [ ] Queue + cap: 2 free clips/day per user, one render at a time (M1 8GB = one concurrent render, queue the rest; 10 users × 2/day ≈ trivial)
- [ ] Privacy note: users' MP4s go to the home MacBook — one honest paragraph on the site
- [ ] **Telegram alerts (server hygiene):**
  - [ ] Create bot via @BotFather (30s, gives a token)
  - [ ] Status script ON THE SPARE: every 6h via launchd → battery %, thermal (`pmset -g therm`), disk, uptime, queue depth
  - [ ] Watchdog ON THE MAIN MacBook (spare can't text you when it's dead): 2-line ping → "⚠️ server is down" alert
- [ ] **Paid tier trigger (do NOT build yet):** when the queue backs up regularly → add $5-10/mo "priority + 20/day" (Stripe, student pack = first $1,000 fee-free). Until then: no checkout, no license system, no support burden.

## 2. OSS credit programs (free frontier tools, ~6 months)

- [ ] Apply: **Claude for Open Source** (claude.com/contact-sales/claude-for-oss) — six months Claude Max 20x for maintainers of meaningful OSS
- [ ] Apply: **Codex for Open Source** (openai.com/form/codex-for-oss) — Codex access for core maintainers; Codex Open Source Fund = grants
- [ ] Expect rejection at current star count — the streaming + stars push is what makes acceptance likelier later; reapplying costs 10 min
- [ ] Note: these are *building* tools (Claude/Codex seats), not API credits to embed in the app's AI selector

## 3. Streaming + distribution (the actual bottleneck)

- [ ] Live stream: building Clipping Factory in public (1-2×/week) — every stream produces ≥2 clips for Shorts/X (million-plan rule)
- [ ] Stream announcement: "I made a podcast clipper, it's free, here's the URL, I'll keep building it live"
- [ ] Video 3: "how to run Clipping Factory" — **1080p, loudnorm to -14 LUFS** (old videos are 360p / ~-30 LUFS)
- [ ] Post the 3 promo videos (`promo-videos/output/`) to X
- [ ] GitHub stars goal: pin repo link everywhere; stars = the credibility asset for the channel + OSS program applications
- [ ] GitHub listing looks like a product: topics (`podcast`, `clipper`, `whisper`, `ffmpeg`, `local-first`), homepage URL, a 20-second drop→clips GIF, tagged `v0.1.0`

### X posting schedule (one post a day, same hour, ~2 min)

Do **one** post every day. Same time (pick one and keep it — morning PT works). Do not do 5/day. The link is always the repo or the hosted URL. Every post is allowed to repeat the offer: it's free, local, no account.

Rotate this 7-day loop forever. When a slot is a video, use a real CF clip or one of `promo-videos/output/`.

| Day | What to post | Example skeleton |
|---|---|---|
| Mon | Demo clip | 15–30s vertical from CF. "Dropped a podcast in. This is what came out. Free, runs on your machine. [link]" |
| Tue | Build in public | One screenshot or one sentence about what shipped (or what's broken). "Added YouTube paste today." / "Two-host shows still letterbox. Fixing that." |
| Wed | How-to | "Paste a YouTube URL or drop an MP4. It finds the moments, captions them, writes 9:16 files. No account. [link]" |
| Thu | Proof / objection | Answer the thing people assume: "Video never leaves your computer. Only optional: you paste your own API key. Default ranker is local." |
| Fri | Recap + ask | "This week: X shipped. Repo: [link]. Star it if you want a local OpusClip that doesn't rewrite the guest." |
| Sat | Stream clip or leftover promo | Cut 2 clips from the week's stream (or post the next unused file in `promo-videos/output/`). Same CTA. |
| Sun | Off or quote-tweet a user | If nobody used it: post the sample-episode result from the empty state. If someone did: amplify that. |

Rules that keep this from becoming a second job:
- Write the post in the same sitting as the work. Do not batch-write a content calendar.
- Pin one post: "I made a free local podcast clipper. [repo] [hosted URL if live]."
- First three posts: the three promo videos, in order (editorial → momentum → quiet), then start the loop.
- Never invent a virality score in the copy. Never claim cloud features we don't have.

## 4. Make the app better

Ranked. Do the **now** block before the rest. Packaging / Windows stay parked until the free pilot has real users.

### Now (user can feel these)

- [ ] **Sample episode on the empty state** — one owned/synthetic ~90s podcast already on the first page, so "try it" does not require their file. Button: "Run the sample."
- [ ] **More input formats** — MOV, MKV, WebM, M4V, plus audio-only (M4A / WAV / MP3). Anything ffmpeg can decode. Drop zone and file picker accept all of them, not just `.mp4`.
- [ ] **Paste a YouTube (or RSS) URL** — yes, this is possible. Local `yt-dlp` downloads the file onto their machine, then the existing pipeline runs. No cloud upload. Reject livestreams / private videos with a clear error. Same permission rule: only content they own or may process.
- [ ] **Project list** — recent projects on the empty state: filename, status, clip count, date, Open / Open folder / Delete. Resume any project, not just the last `localStorage` id. Delete must not touch `~/Downloads/Clipping Factory/`.
- [ ] **Review, then render only what they want** — after ranking, show the candidate list (headline, timestamps, first line, reason) with checkboxes. Default can still be "render the top ones," but they can uncheck everything except one clip and not wait 20 minutes. Render-selected, not render-all.
- [ ] **Fix names in the real transcript** — click a word, type the guest/product name, captions re-burn from the base. Per-show vocabulary list so "Bishesha" is not re-misspelled next episode. Does not change the audio.
- [ ] **Nudged start/end** — trim "so / um / well" openings and snap the end to the next sentence. Plus a small slider so they can drag the cut ± a few seconds without opening another editor.
- [ ] **Clips look postable**
  - [ ] Karaoke + Hormozi caption styles (on top of Impact / Clean)
  - [ ] Caption position (lower / center / upper) and size — TikTok UI covers the bottom
  - [ ] Live caption preview in the browser before the burn
  - [ ] Optional 2s hook card from the verified headline or the first spoken words (default off)
  - [ ] Optional logo + short end card (their PNG, default off)
  - [ ] Loudnorm to **−14 LUFS** on every final clip
  - [ ] Copy-ready title + 1-line description + a few topic tags next to Download
  - [ ] Platform safe-area overlay on the preview (TikTok / Reels / Shorts)

### Agent / local API (their computer, their folder)

An agent (or a script) should be able to drop a video on a machine and get finished clips in a folder. Nothing runs on our servers — the studio already is a local HTTP API.

- [ ] **Import from a local path** — `POST /api/projects/import { "path": "/abs/file.mp4" }` (or a YouTube URL that `yt-dlp` writes next to it). No browser multipart.
- [ ] **Wait + export** — `GET /api/projects/{id}` already exists; add `GET /api/projects/{id}/export.json` (transcript, candidates, manifest, clip paths).
- [ ] **Write clips to a folder they name** — e.g. `CF_OUTPUT_DIR` or `{ "out": "~/Movies/clips" }`. Agent does not scrape the UI.
- [ ] **MCP adapter** (after the JSON routes work) — tools: `create_project`, `start`, `status`, `list_clips`, `download`. Thin shell over the same handlers. Flag it `--enable-mcp`. Loopback only unless the Funnel token is on.

### Also on the list (after the now block)

- [ ] Golden-set evals with real episodes (`evals/sources` empty — harness exists)
- [ ] Speaker-aware framing for two-person podcasts (the real market — most podcasts are 2 hosts)
- [ ] Reaction-aware picks: laugh / loud burst → extend the clip backwards to the setup (model-free first; YAMNet later)
- [ ] Spread clips across the episode (don't dump every clip from one 20-minute stretch)
- [ ] Optional prompt box: "look for disagreements / stories / tactics" — same validator, different emphasis. No virality score.
- [ ] Export the same clip as 1:1 and 16:9 (default stays 9:16)
- [ ] Download-all ZIP + `.srt` next to each MP4 + a small edit-decision JSON
- [ ] Show presets (caption style, color, font, logo, hook, loudness) so episode 2 matches episode 1
- [ ] Sequential queue: drop several files, process one at a time
- [ ] Disk-space guard before heavy stages ("this render needs ~2.1 GB; 1.4 GB free")
- [ ] Per-clip retry (don't rerun the whole pipeline for one failed render)
- [ ] Windows build: LATER, only after the free pilot shows real users

## Done (don't redo)

- App works, CI green (104 tests), server runs via launchd
- Repo consolidated on GitHub (codingwithb/clipping-factory), MIT, promo-videos in-repo
- bishesha.com live (Netlify)
- 9 YouTube commenters replied (2026-08)
- Desktop .dmg idea parked — the free hosted pilot comes first; packaging becomes relevant again only if/when people want local
