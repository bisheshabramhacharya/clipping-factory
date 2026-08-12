import {Easing, Img, Interactive, interpolate, staticFile, useCurrentFrame, useVideoConfig} from 'remotion';

export const palette = {
  ink: '#171715',
  paper: '#F3F1EA',
  orange: '#F47721',
  amber: '#FFB21C',
  mist: '#DCD8CC',
  white: '#FFFDF7',
};

export const Grain: React.FC<{light?: boolean}> = ({light = false}) => {
  const frame = useCurrentFrame();
  return (
    <div
      style={{
        position: 'absolute',
        inset: 0,
        opacity: light ? 0.055 : 0.08,
        backgroundImage: `radial-gradient(circle at ${20 + (frame % 7) * 9}% ${30 + (frame % 5) * 11}%, ${light ? '#171715' : '#FFFDF7'} 0.7px, transparent 0.9px)`,
        backgroundSize: '8px 8px',
        mixBlendMode: light ? 'multiply' : 'screen',
        pointerEvents: 'none',
      }}
    />
  );
};

export const Brand: React.FC<{dark?: boolean}> = ({dark = false}) => (
  <Interactive.Div
    name="Clipping Factory brand"
    style={{
      position: 'absolute',
      left: 84,
      top: 92,
      display: 'flex',
      alignItems: 'center',
      gap: 18,
      color: dark ? palette.paper : palette.ink,
      fontFamily: 'Inter',
      fontSize: 28,
      fontWeight: 700,
      letterSpacing: -1,
    }}
  >
    <span style={{width: 16, height: 16, borderRadius: 99, backgroundColor: palette.orange}} />
    Clipping Factory
  </Interactive.Div>
);

export const Eyebrow: React.FC<{children: React.ReactNode; dark?: boolean}> = ({children, dark = false}) => {
  const frame = useCurrentFrame();
  return (
    <Interactive.Div
      name="Eyebrow"
      style={{
        color: dark ? palette.paper : palette.ink,
        fontFamily: 'Inter',
        fontSize: 24,
        fontWeight: 700,
        letterSpacing: 5,
        textTransform: 'uppercase',
        opacity: interpolate(frame, [0, 14], [0, 0.72], {
          extrapolateLeft: 'clamp',
          extrapolateRight: 'clamp',
          easing: Easing.bezier(0.16, 1, 0.3, 1),
        }),
      }}
    >
      {children}
    </Interactive.Div>
  );
};

export const ProductFrame: React.FC<{
  src: 'input.png' | 'results.png' | 'candidates.png';
  width?: number;
  height?: number;
  top?: number;
  left?: number;
  imageScale?: number;
  imageX?: number;
  imageY?: number;
  rotate?: number;
  dark?: boolean;
}> = ({src, width = 890, height = 760, top = 800, left = 95, imageScale = 1, imageX = 0, imageY = 0, rotate = 0, dark = false}) => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  return (
    <Interactive.Div
      name="Product screenshot"
      style={{
        position: 'absolute',
        left,
        top,
        width,
        height,
        overflow: 'hidden',
        borderRadius: 36,
        border: `1px solid ${dark ? 'rgba(255,253,247,0.22)' : 'rgba(23,23,21,0.18)'}`,
        backgroundColor: palette.white,
        boxShadow: dark ? '0 42px 110px rgba(0,0,0,0.38)' : '0 42px 100px rgba(23,23,21,0.16)',
        rotate: `${rotate}deg`,
        opacity: interpolate(frame, [0, 0.28 * fps], [0, 1], {
          extrapolateLeft: 'clamp',
          extrapolateRight: 'clamp',
          easing: Easing.bezier(0.16, 1, 0.3, 1),
        }),
        translate: interpolate(frame, [0, 0.5 * fps], ['0px 86px', '0px 0px'], {
          extrapolateLeft: 'clamp',
          extrapolateRight: 'clamp',
          easing: Easing.spring({damping: 200}),
        }),
        scale: interpolate(frame, [0, 0.7 * fps], [0.96, 1], {
          extrapolateLeft: 'clamp',
          extrapolateRight: 'clamp',
          easing: Easing.spring({damping: 200}),
          output: 'perceptual-scale',
        }),
      }}
    >
      <Img
        name="Clipping Factory UI"
        src={staticFile(`screenshots/${src}`)}
        style={{
          width: '100%',
          height: '100%',
          objectFit: 'cover',
          objectPosition: 'center top',
          scale: imageScale,
          translate: `${imageX}px ${imageY}px`,
        }}
      />
      <div style={{position: 'absolute', inset: 0, boxShadow: 'inset 0 0 0 1px rgba(255,255,255,0.45)', borderRadius: 36}} />
    </Interactive.Div>
  );
};

export const FooterLine: React.FC<{dark?: boolean; text?: string}> = ({dark = false, text = 'Free · Local-first · No account'}) => (
  <Interactive.Div
    name="Footer benefit line"
    style={{
      position: 'absolute',
      left: 84,
      right: 84,
      bottom: 86,
      borderTop: `1px solid ${dark ? 'rgba(255,253,247,0.24)' : 'rgba(23,23,21,0.22)'}`,
      paddingTop: 28,
      color: dark ? palette.paper : palette.ink,
      fontFamily: 'Inter',
      fontSize: 24,
      fontWeight: 600,
      letterSpacing: 1,
      opacity: 0.78,
    }}
  >
    {text}
  </Interactive.Div>
);

export const StepPill: React.FC<{index: string; label: string; active?: boolean}> = ({index, label, active = false}) => {
  const frame = useCurrentFrame();
  return (
    <Interactive.Div
      name={`Step ${index}`}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 22,
        padding: '22px 28px',
        borderRadius: 99,
        backgroundColor: active ? palette.orange : 'rgba(255,253,247,0.08)',
        border: `1px solid ${active ? palette.orange : 'rgba(255,253,247,0.18)'}`,
        color: active ? palette.ink : palette.paper,
        fontFamily: 'Inter',
        fontSize: 27,
        fontWeight: 700,
        opacity: interpolate(frame, [0, 14], [0, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'}),
        translate: interpolate(frame, [0, 18], ['-26px 0px', '0px 0px'], {
          extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: Easing.spring({damping: 200}),
        }),
      }}
    >
      <span style={{opacity: 0.62}}>{index}</span>
      {label}
    </Interactive.Div>
  );
};
