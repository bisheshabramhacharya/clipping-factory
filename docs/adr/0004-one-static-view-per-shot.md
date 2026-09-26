# One static view per shot, cut to the speaker

Supersedes the single-position rule of ADR-0001 and relaxes ADR-0002 for zoomed views.

ADR-0001 locked each clip to one crop position for its whole length. On multi-camera podcasts, which is most of them, the source cuts between cameras every few seconds, so after the first cut the locked window pointed at wherever the face used to be: an empty chair, the table, or the back of someone's head. The full-height window also left people in wide shots small, with the table filling the lower half.

Decision:

- A clip is split into shots at the source's camera cuts (`scdet` over the clip at full frame rate). Each shot gets its own static view, and the render joins the views with hard cuts on the same frames the source cut on.
- Within a shot, detections are pooled across frames: the camera doesn't move within a shot, so a face the detector finds in about a third of the frames is framed for the whole shot.
- A shot with one person is cropped on them. With several people, the view goes to the one whose mouth moves most while words are spoken; the view only switches people on turns long enough to beat a switching cost (about 2 s), and the switch lands on the first word of the turn.
- A shot with no usable face shows the full frame over a blurred copy (the BlurPad treatment) instead of a crop.
- A view may zoom in past the full-height window until the face box is about 20% of the frame height, up to 1.6× and never to a window under 540 source pixels tall. This is the one place output pixels are upscaled (ADR-0002's canvas size is unchanged). Zoomed views place the eyes about 36% down the frame.
- The camera still never pans, eases, or tracks within a view. ADR-0001's no-motion rule stands.

The eye-line slide over a blurred underlay is removed: it put a blurred band at the top or bottom of a full-height crop. Full-height views stay pixel-exact crops.
