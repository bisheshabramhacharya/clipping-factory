import {AbsoluteFill, Easing, Interactive, interpolate, useCurrentFrame, useVideoConfig} from 'remotion';
import {Brand, Grain, ProductFrame, palette} from '../../design';

export const MomentumHook: React.FC = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  return (
    <AbsoluteFill style={{backgroundColor: palette.ink, overflow: 'hidden'}}>
      <Brand dark />
      <Interactive.Div
        name="Momentum hook"
        style={{
          position: 'absolute', left: 76, right: 52, top: 288,
          color: palette.paper, fontFamily: 'Inter', fontSize: 120, fontWeight: 800, lineHeight: 0.91, letterSpacing: -8,
          opacity: interpolate(frame, [0, 0.45 * fps], [0, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.bezier(0.16, 1, 0.3, 1)}),
          translate: interpolate(frame, [0, 0.7 * fps], ['-72px 0px', '0px 0px'], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.spring({damping: 190})}),
        }}
      >
        Your podcast<br />already has<br /><span style={{color: palette.orange}}>the clips.</span>
      </Interactive.Div>
      <ProductFrame src="input.png" top={1030} left={130} width={820} height={580} rotate={-2.5} dark imageScale={1.08} />
      <Interactive.Div
        name="Progress line"
        style={{position: 'absolute', left: 84, bottom: 110, height: 8, borderRadius: 99, backgroundColor: palette.orange, width: interpolate(frame, [0, 2.8 * fps], [0, 912], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.bezier(0.16, 1, 0.3, 1)})}}
      />
      <Grain />
    </AbsoluteFill>
  );
};
