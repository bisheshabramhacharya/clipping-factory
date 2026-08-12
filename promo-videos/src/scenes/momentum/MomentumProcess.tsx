import {AbsoluteFill, Interactive} from 'remotion';
import {Brand, Grain, ProductFrame, StepPill, palette} from '../../design';

export const MomentumProcess: React.FC = () => (
  <AbsoluteFill style={{backgroundColor: palette.orange, overflow: 'hidden'}}>
    <Brand />
    <Interactive.Div
      name="Process title"
      style={{position: 'absolute', left: 76, right: 76, top: 278, color: palette.ink, fontFamily: 'Inter', fontSize: 94, fontWeight: 800, lineHeight: 0.98, letterSpacing: -5}}
    >
      From full episode<br />to feed-ready.
    </Interactive.Div>
    <div style={{position: 'absolute', left: 76, right: 76, top: 590, display: 'flex', flexDirection: 'column', gap: 16}}>
      <StepPill index="01" label="Inspect the source" />
      <StepPill index="02" label="Transcribe every word" />
      <StepPill index="03" label="Rank the moments" active />
      <StepPill index="04" label="Render vertical clips" />
    </div>
    <ProductFrame src="results.png" top={1110} left={150} width={780} height={520} imageScale={1.25} imageY={-70} rotate={2.5} />
    <Grain light />
  </AbsoluteFill>
);
