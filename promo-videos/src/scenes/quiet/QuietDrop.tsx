import {AbsoluteFill, Easing, Interactive, interpolate, useCurrentFrame} from 'remotion';
import {Brand, Grain, ProductFrame, palette} from '../../design';

export const QuietDrop: React.FC = () => {
  const frame = useCurrentFrame();
  return (
    <AbsoluteFill style={{backgroundColor: palette.paper, overflow: 'hidden'}}>
      <Brand />
      <Interactive.Div name="Step number" style={{position: 'absolute', left: 84, top: 300, color: palette.orange, fontFamily: 'Inter', fontSize: 30, fontWeight: 800, letterSpacing: 4}}>01 / INPUT</Interactive.Div>
      <Interactive.Div
        name="Drop instruction"
        style={{
          position: 'absolute', left: 84, right: 84, top: 380, color: palette.ink, fontFamily: 'Inter', fontSize: 88, fontWeight: 800, lineHeight: 1, letterSpacing: -5,
          opacity: interpolate(frame, [0, 18], [0, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'}),
          translate: interpolate(frame, [0, 28], ['0px 42px', '0px 0px'], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.spring({damping: 220})}),
        }}
      >
        Drop one<br />podcast MP4.
      </Interactive.Div>
      <ProductFrame src="input.png" top={740} height={840} imageScale={1.14} />
      <Interactive.Div name="Privacy note" style={{position: 'absolute', left: 84, bottom: 96, color: palette.ink, fontFamily: 'Inter', fontSize: 29, fontWeight: 600, opacity: 0.6}}>The video stays on your computer.</Interactive.Div>
      <Grain light />
    </AbsoluteFill>
  );
};
