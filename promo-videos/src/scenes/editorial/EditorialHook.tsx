import {AbsoluteFill, Easing, Interactive, interpolate, useCurrentFrame, useVideoConfig} from 'remotion';
import {Brand, Eyebrow, FooterLine, Grain, ProductFrame, palette} from '../../design';

export const EditorialHook: React.FC = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  return (
    <AbsoluteFill style={{backgroundColor: palette.paper, overflow: 'hidden'}}>
      <Brand />
      <div style={{position: 'absolute', left: 84, right: 70, top: 300}}>
        <Eyebrow>A better way to clip</Eyebrow>
        <Interactive.Div
          name="Hero headline"
          style={{
            marginTop: 34,
            color: palette.ink,
            fontFamily: 'Inter',
            fontSize: 112,
            fontWeight: 800,
            lineHeight: 0.96,
            letterSpacing: -7,
            opacity: interpolate(frame, [6, 0.5 * fps], [0, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.bezier(0.16, 1, 0.3, 1)}),
            translate: interpolate(frame, [6, 0.7 * fps], ['0px 54px', '0px 0px'], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.spring({damping: 200})}),
          }}
        >
          One podcast.<br />The right clips.
        </Interactive.Div>
        <Interactive.Div
          name="Hero support"
          style={{
            marginTop: 46,
            maxWidth: 780,
            color: palette.ink,
            fontFamily: 'Inter',
            fontSize: 38,
            fontWeight: 600,
            lineHeight: 1.3,
            opacity: interpolate(frame, [0.6 * fps, 1.1 * fps], [0, 0.68], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'}),
          }}
        >
          Drop in the conversation. Keep the moments that stand on their own.
        </Interactive.Div>
      </div>
      <ProductFrame src="input.png" top={960} height={650} imageScale={1.02} />
      <FooterLine />
      <Grain light />
    </AbsoluteFill>
  );
};
