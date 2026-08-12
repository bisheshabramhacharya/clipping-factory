import {Audio} from '@remotion/media';
import {TransitionSeries, linearTiming} from '@remotion/transitions';
import {fade} from '@remotion/transitions/fade';
import {slide} from '@remotion/transitions/slide';
import {AbsoluteFill, interpolate, staticFile} from 'remotion';
import {QuietHook} from '../scenes/quiet/QuietHook';
import {QuietDrop} from '../scenes/quiet/QuietDrop';
import {QuietReview} from '../scenes/quiet/QuietReview';
import {QuietEnd} from '../scenes/quiet/QuietEnd';

export const Quiet: React.FC = () => (
  <AbsoluteFill>
    <Audio src={staticFile('music/quiet.wav')} volume={(f) => interpolate(f, [0, 28, 405, 460], [0, 0.88, 0.88, 0], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'})} />
    <TransitionSeries>
      <TransitionSeries.Sequence durationInFrames={120} name="Hook"><QuietHook /></TransitionSeries.Sequence>
      <TransitionSeries.Transition presentation={fade()} timing={linearTiming({durationInFrames: 20})} />
      <TransitionSeries.Sequence durationInFrames={140} name="Drop"><QuietDrop /></TransitionSeries.Sequence>
      <TransitionSeries.Transition presentation={slide({direction: 'from-bottom'})} timing={linearTiming({durationInFrames: 20})} />
      <TransitionSeries.Sequence durationInFrames={140} name="Review"><QuietReview /></TransitionSeries.Sequence>
      <TransitionSeries.Transition presentation={fade()} timing={linearTiming({durationInFrames: 20})} />
      <TransitionSeries.Sequence durationInFrames={120} name="End card"><QuietEnd /></TransitionSeries.Sequence>
    </TransitionSeries>
  </AbsoluteFill>
);
