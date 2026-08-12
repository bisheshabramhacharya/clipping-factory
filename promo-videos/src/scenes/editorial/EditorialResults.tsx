import {AbsoluteFill, Easing, Interactive, interpolate, useCurrentFrame} from 'remotion';
import {Brand, FooterLine, Grain, ProductFrame, palette} from '../../design';

export const EditorialResults: React.FC = () => {
  const frame = useCurrentFrame();
  return (
    <AbsoluteFill style={{backgroundColor: palette.paper, overflow: 'hidden'}}>
      <Brand />
      <Interactive.Div
        name="Results count"
        style={{
          position: 'absolute', left: 84, top: 286,
          color: palette.orange, fontFamily: 'Inter', fontSize: 184, fontWeight: 800, lineHeight: 0.8, letterSpacing: -12,
          opacity: interpolate(frame, [0, 16], [0, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'}),
          scale: interpolate(frame, [0, 24], [0.86, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.spring({damping: 180}), output: 'perceptual-scale'}),
        }}
      >
        5
      </Interactive.Div>
      <Interactive.Div
        name="Results headline"
        style={{position: 'absolute', left: 84, top: 450, right: 84, color: palette.ink, fontFamily: 'Inter', fontSize: 82, fontWeight: 800, lineHeight: 1.02, letterSpacing: -4}}
      >
        strong clips.<br />Already ranked.
      </Interactive.Div>
      <ProductFrame src="candidates.png" top={790} height={780} imageScale={1.18} imageY={-120} />
      <FooterLine text="Faithful excerpts · Face-tracked framing · Word-accurate captions" />
      <Grain light />
    </AbsoluteFill>
  );
};
