import {AbsoluteFill, Easing, Interactive, interpolate, useCurrentFrame, useVideoConfig} from 'remotion';
import {Brand, Grain, palette} from '../../design';

export const EditorialEnd: React.FC = () => {
  const frame = useCurrentFrame();
  const {durationInFrames} = useVideoConfig();
  return (
    <AbsoluteFill style={{backgroundColor: palette.ink, display: 'flex', alignItems: 'center', justifyContent: 'center'}}>
      <Brand dark />
      <div style={{position: 'absolute', left: 84, right: 84, top: 510}}>
        <Interactive.Div
          name="Free headline"
          style={{
            color: palette.orange, fontFamily: 'Inter', fontSize: 210, fontWeight: 800, letterSpacing: -14, lineHeight: 0.85,
            opacity: interpolate(frame, [0, 18, durationInFrames - 18, durationInFrames], [0, 1, 1, 0], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'}),
            scale: interpolate(frame, [0, 30], [0.78, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.spring({damping: 180}), output: 'perceptual-scale'}),
          }}
        >
          Free.
        </Interactive.Div>
        <Interactive.Div
          name="Final promise"
          style={{marginTop: 70, color: palette.paper, fontFamily: 'Inter', fontSize: 62, fontWeight: 800, lineHeight: 1.08, letterSpacing: -3}}
        >
          One podcast in.<br />Every strong, faithful clip out.
        </Interactive.Div>
        <Interactive.Div
          name="Final support"
          style={{marginTop: 48, color: palette.paper, fontFamily: 'Inter', fontSize: 30, fontWeight: 600, opacity: 0.62}}
        >
          Local-first. No account. Built for your Mac.
        </Interactive.Div>
      </div>
      <div style={{position: 'absolute', left: 84, bottom: 110, width: 120, height: 8, borderRadius: 99, backgroundColor: palette.orange}} />
      <Grain />
    </AbsoluteFill>
  );
};
