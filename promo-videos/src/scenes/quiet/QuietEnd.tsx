import {AbsoluteFill, Easing, Interactive, interpolate, useCurrentFrame} from 'remotion';
import {Brand, Grain, palette} from '../../design';

export const QuietEnd: React.FC = () => {
  const frame = useCurrentFrame();
  return (
    <AbsoluteFill style={{backgroundColor: palette.white}}>
      <Brand />
      <Interactive.Div name="Step number" style={{position: 'absolute', left: 84, top: 300, color: palette.orange, fontFamily: 'Inter', fontSize: 30, fontWeight: 800, letterSpacing: 4}}>03 / DOWNLOAD</Interactive.Div>
      <Interactive.Div
        name="Download headline"
        style={{
          position: 'absolute', left: 84, right: 70, top: 410, color: palette.ink, fontFamily: 'Inter', fontSize: 106, fontWeight: 800, lineHeight: 0.96, letterSpacing: -7,
          opacity: interpolate(frame, [0, 20], [0, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'}),
          scale: interpolate(frame, [0, 30], [0.92, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.spring({damping: 220}), output: 'perceptual-scale'}),
        }}
      >
        Download the<br />clips worth posting.
      </Interactive.Div>
      <div style={{position: 'absolute', left: 84, right: 84, top: 890, borderTop: `2px solid ${palette.mist}`}} />
      <Interactive.Div name="Free statement" style={{position: 'absolute', left: 84, top: 970, color: palette.orange, fontFamily: 'Inter', fontSize: 120, fontWeight: 800, letterSpacing: -7}}>Free.</Interactive.Div>
      <Interactive.Div name="Final benefits" style={{position: 'absolute', left: 84, top: 1140, color: palette.ink, fontFamily: 'Inter', fontSize: 48, fontWeight: 800, lineHeight: 1.45}}>Local-first.<br />Faithful excerpts.<br />Finished vertical clips.</Interactive.Div>
      <Interactive.Div name="Final tagline" style={{position: 'absolute', left: 84, bottom: 112, color: palette.ink, fontFamily: 'Inter', fontSize: 28, fontWeight: 700, opacity: 0.62}}>Clipping Factory · One podcast in. Every strong clip out.</Interactive.Div>
      <Grain light />
    </AbsoluteFill>
  );
};
