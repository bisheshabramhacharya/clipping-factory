import {Audio} from '@remotion/media';
import {TransitionSeries, linearTiming} from '@remotion/transitions';
import {fade} from '@remotion/transitions/fade';
import {slide} from '@remotion/transitions/slide';
import {AbsoluteFill, interpolate, staticFile} from 'remotion';
import {MomentumHook} from '../scenes/momentum/MomentumHook';
import {MomentumProcess} from '../scenes/momentum/MomentumProcess';
import {MomentumRanked} from '../scenes/momentum/MomentumRanked';
import {MomentumEnd} from '../scenes/momentum/MomentumEnd';

export const Momentum: React.FC = () => (
  <AbsoluteFill>
    <Audio src={staticFile('music/momentum.wav')} volume={(f) => interpolate(f, [0, 16, 420, 465], [0, 0.78, 0.78, 0], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'})} />
    <TransitionSeries>
      <TransitionSeries.Sequence durationInFrames={120} name="Hook"><MomentumHook /></TransitionSeries.Sequence>
      <TransitionSeries.Transition presentation={slide({direction: 'from-right'})} timing={linearTiming({durationInFrames: 15})} />
      <TransitionSeries.Sequence durationInFrames={135} name="Process"><MomentumProcess /></TransitionSeries.Sequence>
      <TransitionSeries.Transition presentation={fade()} timing={linearTiming({durationInFrames: 15})} />
      <TransitionSeries.Sequence durationInFrames={135} name="Ranked"><MomentumRanked /></TransitionSeries.Sequence>
      <TransitionSeries.Transition presentation={slide({direction: 'from-bottom'})} timing={linearTiming({durationInFrames: 15})} />
      <TransitionSeries.Sequence durationInFrames={120} name="End card"><MomentumEnd /></TransitionSeries.Sequence>
    </TransitionSeries>
  </AbsoluteFill>
);
