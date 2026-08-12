import {AbsoluteFill, Easing, Interactive, interpolate, useCurrentFrame} from 'remotion';
import {Brand, Grain, ProductFrame, palette} from '../../design';

export const MomentumRanked: React.FC = () => {
  const frame = useCurrentFrame();
  return (
    <AbsoluteFill style={{backgroundColor: palette.ink, overflow: 'hidden'}}>
      <Brand dark />
      <Interactive.Div
        name="Ranked label"
        style={{position: 'absolute', left: 76, top: 280, color: palette.orange, fontFamily: 'Inter', fontSize: 26, fontWeight: 800, letterSpacing: 5, textTransform: 'uppercase'}}
      >
        Selected, not spammed
      </Interactive.Div>
      <Interactive.Div
        name="Ranked headline"
        style={{
          position: 'absolute', left: 76, right: 64, top: 350, color: palette.paper, fontFamily: 'Inter', fontSize: 104, fontWeight: 800, lineHeight: 0.96, letterSpacing: -6,
          opacity: interpolate(frame, [0, 16], [0, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'}),
          translate: interpolate(frame, [0, 28], ['0px 58px', '0px 0px'], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.spring({damping: 200})}),
        }}
      >
        Ranked for<br />a reason.
      </Interactive.Div>
      <Interactive.Div
        name="Ranked support"
        style={{position: 'absolute', left: 76, right: 76, top: 590, color: palette.paper, fontFamily: 'Inter', fontSize: 34, fontWeight: 600, lineHeight: 1.32, opacity: 0.62}}
      >
        Strong openings. Clear payoff. One faithful excerpt.
      </Interactive.Div>
      <ProductFrame src="candidates.png" top={790} left={110} width={860} height={850} imageScale={1.28} imageY={-100} dark />
      <Grain />
    </AbsoluteFill>
  );
};
