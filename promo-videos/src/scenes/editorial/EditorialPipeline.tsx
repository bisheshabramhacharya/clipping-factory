import {AbsoluteFill, Easing, Interactive, interpolate, useCurrentFrame, useVideoConfig} from 'remotion';
import {Brand, Grain, ProductFrame, StepPill, palette} from '../../design';

export const EditorialPipeline: React.FC = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  return (
    <AbsoluteFill style={{backgroundColor: palette.ink, overflow: 'hidden'}}>
      <Brand dark />
      <Interactive.Div
        name="Pipeline title"
        style={{
          position: 'absolute', left: 84, right: 84, top: 286,
          color: palette.paper, fontFamily: 'Inter', fontSize: 94, fontWeight: 800, lineHeight: 0.98, letterSpacing: -5,
          opacity: interpolate(frame, [0, 18], [0, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.bezier(0.16, 1, 0.3, 1)}),
          translate: interpolate(frame, [0, 0.7 * fps], ['0px 48px', '0px 0px'], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.spring({damping: 200})}),
        }}
      >
        It does the<br />tedious part.
      </Interactive.Div>
      <div style={{position: 'absolute', left: 84, right: 84, top: 560, display: 'flex', gap: 14, flexWrap: 'wrap'}}>
        <StepPill index="01" label="Inspect" />
        <StepPill index="02" label="Transcribe" />
        <StepPill index="03" label="Find moments" active />
        <StepPill index="04" label="Render" />
      </div>
      <ProductFrame src="results.png" top={890} height={700} imageScale={1.2} imageY={-90} dark />
      <Interactive.Div
        name="Pipeline note"
        style={{position: 'absolute', left: 84, right: 84, bottom: 94, color: palette.paper, fontFamily: 'Inter', fontSize: 28, fontWeight: 600, opacity: 0.64}}
      >
        Local ranking finds strong openings, tension, and clean payoffs.
      </Interactive.Div>
      <Grain />
    </AbsoluteFill>
  );
};
