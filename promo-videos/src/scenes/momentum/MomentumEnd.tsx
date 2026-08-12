import {AbsoluteFill, Easing, Interactive, interpolate, useCurrentFrame} from 'remotion';
import {Brand, Grain, palette} from '../../design';

export const MomentumEnd: React.FC = () => {
  const frame = useCurrentFrame();
  return (
    <AbsoluteFill style={{backgroundColor: palette.orange, overflow: 'hidden'}}>
      <Brand />
      <Interactive.Div
        name="Stop scrubbing"
        style={{
          position: 'absolute', left: 76, right: 64, top: 370, color: palette.ink, fontFamily: 'Inter', fontSize: 122, fontWeight: 800, lineHeight: 0.9, letterSpacing: -8,
          opacity: interpolate(frame, [0, 16], [0, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'}),
          scale: interpolate(frame, [0, 25], [0.9, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.spring({damping: 200}), output: 'perceptual-scale'}),
        }}
      >
        Stop<br />scrubbing.<br /><span style={{color: palette.white}}>Start posting.</span>
      </Interactive.Div>
      <div style={{position: 'absolute', left: 76, right: 76, top: 980, height: 2, backgroundColor: 'rgba(23,23,21,0.25)'}} />
      <Interactive.Div
        name="Free local footer"
        style={{position: 'absolute', left: 76, right: 76, top: 1040, color: palette.ink, fontFamily: 'Inter', fontSize: 44, fontWeight: 800, lineHeight: 1.25}}
      >
        Clipping Factory<br />Free. Local-first. No account.
      </Interactive.Div>
      <Interactive.Div
        name="Tagline"
        style={{position: 'absolute', left: 76, bottom: 112, color: palette.ink, fontFamily: 'Inter', fontSize: 28, fontWeight: 700, opacity: 0.68}}
      >
        One podcast in. Every strong, faithful clip out.
      </Interactive.Div>
      <Grain light />
    </AbsoluteFill>
  );
};
