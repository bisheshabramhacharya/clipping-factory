import {Audio} from '@remotion/media';
import {TransitionSeries, linearTiming} from '@remotion/transitions';
import {fade} from '@remotion/transitions/fade';
import {slide} from '@remotion/transitions/slide';
import {AbsoluteFill, interpolate, staticFile} from 'remotion';
import {EditorialHook} from '../scenes/editorial/EditorialHook';
import {EditorialPipeline} from '../scenes/editorial/EditorialPipeline';
import {EditorialResults} from '../scenes/editorial/EditorialResults';
import {EditorialEnd} from '../scenes/editorial/EditorialEnd';

export const Editorial: React.FC = () => (
  <AbsoluteFill>
    <Audio
      src={staticFile('music/editorial.wav')}
      volume={(f) => interpolate(f, [0, 20, 390, 435], [0, 0.82, 0.82, 0], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'})}
    />
    <TransitionSeries>
      <TransitionSeries.Sequence durationInFrames={105} name="Hook">
        <EditorialHook />
      </TransitionSeries.Sequence>
      <TransitionSeries.Transition presentation={fade()} timing={linearTiming({durationInFrames: 15})} />
      <TransitionSeries.Sequence durationInFrames={130} name="Pipeline">
        <EditorialPipeline />
      </TransitionSeries.Sequence>
      <TransitionSeries.Transition presentation={slide({direction: 'from-bottom'})} timing={linearTiming({durationInFrames: 15})} />
      <TransitionSeries.Sequence durationInFrames={125} name="Results">
        <EditorialResults />
      </TransitionSeries.Sequence>
      <TransitionSeries.Transition presentation={fade()} timing={linearTiming({durationInFrames: 15})} />
      <TransitionSeries.Sequence durationInFrames={120} name="End card">
        <EditorialEnd />
      </TransitionSeries.Sequence>
    </TransitionSeries>
  </AbsoluteFill>
);
