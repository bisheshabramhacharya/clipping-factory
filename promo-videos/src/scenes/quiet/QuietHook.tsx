import {AbsoluteFill, Easing, Interactive, interpolate, useCurrentFrame} from 'remotion';
import {Brand, FooterLine, Grain, palette} from '../../design';

export const QuietHook: React.FC = () => {
  const frame = useCurrentFrame();
  return (
    <AbsoluteFill style={{backgroundColor: palette.white}}>
      <Brand />
      <div style={{position: 'absolute', left: 84, right: 84, top: 480}}>
        <Interactive.Div
          name="Quiet headline"
          style={{
            color: palette.ink, fontFamily: 'Inter', fontSize: 106, fontWeight: 800, lineHeight: 0.98, letterSpacing: -6,
            opacity: interpolate(frame, [0, 24], [0, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.bezier(0.16, 1, 0.3, 1)}),
            translate: interpolate(frame, [0, 32], ['0px 54px', '0px 0px'], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.spring({damping: 220})}),
          }}
        >
          Clipping should<br />feel this simple.
        </Interactive.Div>
        <Interactive.Div
          name="Quiet support"
          style={{marginTop: 62, color: palette.ink, fontFamily: 'Inter', fontSize: 38, fontWeight: 600, lineHeight: 1.35, opacity: 0.58}}
        >
          No timeline. No cloud upload.<br />No pile of throwaway clips.
        </Interactive.Div>
      </div>
      <div style={{position: 'absolute', left: 84, top: 1150, width: 120, height: 10, borderRadius: 99, backgroundColor: palette.orange}} />
      <FooterLine />
      <Grain light />
    </AbsoluteFill>
  );
};
