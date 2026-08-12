import {AbsoluteFill, Easing, Interactive, interpolate, useCurrentFrame} from 'remotion';
import {Brand, Grain, ProductFrame, palette} from '../../design';

export const QuietReview: React.FC = () => {
  const frame = useCurrentFrame();
  return (
    <AbsoluteFill style={{backgroundColor: palette.paper, overflow: 'hidden'}}>
      <Brand />
      <Interactive.Div name="Step number" style={{position: 'absolute', left: 84, top: 300, color: palette.orange, fontFamily: 'Inter', fontSize: 30, fontWeight: 800, letterSpacing: 4}}>02 / REVIEW</Interactive.Div>
      <Interactive.Div
        name="Review instruction"
        style={{
          position: 'absolute', left: 84, right: 84, top: 380, color: palette.ink, fontFamily: 'Inter', fontSize: 88, fontWeight: 800, lineHeight: 1, letterSpacing: -5,
          opacity: interpolate(frame, [0, 18], [0, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'}),
          translate: interpolate(frame, [0, 28], ['0px 42px', '0px 0px'], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.spring({damping: 220})}),
        }}
      >
        Review the<br />moments that work.
      </Interactive.Div>
      <ProductFrame src="results.png" top={740} height={840} imageScale={1.14} imageY={-40} />
      <Interactive.Div name="Ranking note" style={{position: 'absolute', left: 84, bottom: 96, color: palette.ink, fontFamily: 'Inter', fontSize: 29, fontWeight: 600, opacity: 0.6}}>Ranked by opening, tension, payoff, and clarity.</Interactive.Div>
      <Grain light />
    </AbsoluteFill>
  );
};
