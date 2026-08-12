import {Composition, Folder} from 'remotion';
import './fonts';
import {Editorial} from './videos/Editorial';
import {Momentum} from './videos/Momentum';
import {Quiet} from './videos/Quiet';

export const RemotionRoot: React.FC = () => {
  return (
    <Folder name="Clipping-Factory-Intros">
      <Composition
        id="ClippingFactory-Editorial"
        component={Editorial}
        durationInFrames={435}
        fps={30}
        width={1080}
        height={1920}
      />
      <Composition
        id="ClippingFactory-Momentum"
        component={Momentum}
        durationInFrames={465}
        fps={30}
        width={1080}
        height={1920}
      />
      <Composition
        id="ClippingFactory-Quiet"
        component={Quiet}
        durationInFrames={460}
        fps={30}
        width={1080}
        height={1920}
      />
    </Folder>
  );
};
