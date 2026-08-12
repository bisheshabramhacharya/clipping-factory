import {mkdir, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';

const sampleRate = 48000;
const channels = 2;
const seconds = 18;

const midi = (note) => 440 * 2 ** ((note - 69) / 12);
const clamp = (n) => Math.max(-1, Math.min(1, n));
const env = (t, start, length, attack = 0.018, release = 0.16) => {
  const local = t - start;
  if (local < 0 || local >= length) return 0;
  return Math.min(1, local / attack, (length - local) / release);
};

const writeWav = async (name, makeSample) => {
  const frames = sampleRate * seconds;
  const dataBytes = frames * channels * 2;
  const out = Buffer.alloc(44 + dataBytes);
  out.write('RIFF', 0);
  out.writeUInt32LE(36 + dataBytes, 4);
  out.write('WAVE', 8);
  out.write('fmt ', 12);
  out.writeUInt32LE(16, 16);
  out.writeUInt16LE(1, 20);
  out.writeUInt16LE(channels, 22);
  out.writeUInt32LE(sampleRate, 24);
  out.writeUInt32LE(sampleRate * channels * 2, 28);
  out.writeUInt16LE(channels * 2, 32);
  out.writeUInt16LE(16, 34);
  out.write('data', 36);
  out.writeUInt32LE(dataBytes, 40);

  for (let i = 0; i < frames; i++) {
    const t = i / sampleRate;
    const [left, right] = makeSample(t);
    out.writeInt16LE(Math.round(clamp(left) * 32767), 44 + i * 4);
    out.writeInt16LE(Math.round(clamp(right) * 32767), 46 + i * 4);
  }

  await writeFile(resolve('public/music', name), out);
};

const softKick = (t, at) => {
  const x = t - at;
  if (x < 0 || x > 0.42) return 0;
  return Math.sin(2 * Math.PI * (52 + 46 * Math.exp(-x * 24)) * x) * Math.exp(-x * 12);
};

const softHat = (t, at) => {
  const x = t - at;
  if (x < 0 || x > 0.08) return 0;
  const noise = Math.sin((t * 19481.7) % (Math.PI * 2)) + Math.sin((t * 8711.3) % (Math.PI * 2));
  return noise * 0.5 * Math.exp(-x * 48);
};

const pluck = (t, at, note, length = 0.42) => {
  const e = env(t, at, length, 0.008, 0.2);
  const x = t - at;
  if (!e) return 0;
  const f = midi(note);
  return (Math.sin(2 * Math.PI * f * x) + 0.25 * Math.sin(2 * Math.PI * f * 2 * x)) * e;
};

const pad = (t, at, notes, length) => {
  const e = env(t, at, length, 0.28, 0.5);
  if (!e) return 0;
  const x = t - at;
  return notes.reduce((sum, note) => sum + Math.sin(2 * Math.PI * midi(note) * x), 0) / notes.length * e;
};

await mkdir(resolve('public/music'), {recursive: true});

// Editorial: warm, assured pulse in A minor.
await writeWav('editorial.wav', (t) => {
  const beat = 60 / 104;
  const bar = beat * 4;
  const chords = [[45, 52, 57], [41, 48, 53], [36, 43, 48], [43, 50, 55]];
  let s = 0;
  for (let b = 0; b < Math.ceil(seconds / bar); b++) {
    const at = b * bar;
    s += pad(t, at, chords[b % chords.length], bar + 0.2) * 0.2;
    for (let k = 0; k < 4; k++) s += softKick(t, at + k * beat) * 0.23;
    for (let k = 0; k < 8; k++) s += softHat(t, at + k * beat / 2) * 0.035;
    const melody = [69, 72, 76, 72, 67, 69, 72, 74];
    for (let k = 0; k < 8; k++) s += pluck(t, at + k * beat / 2, melody[(b * 2 + k) % melody.length], beat * 0.42) * 0.09;
  }
  const fade = Math.min(1, t / 0.5, (seconds - t) / 0.8);
  return [s * fade, s * fade * 0.96];
});

// Momentum: brighter digital plucks and a tighter forward rhythm.
await writeWav('momentum.wav', (t) => {
  const beat = 60 / 118;
  const bar = beat * 4;
  const bass = [40, 40, 43, 36, 40, 47, 43, 36];
  const motif = [64, 67, 71, 74, 71, 67, 76, 74];
  let s = 0;
  for (let b = 0; b < Math.ceil(seconds / bar); b++) {
    const at = b * bar;
    for (let k = 0; k < 4; k++) {
      s += softKick(t, at + k * beat) * (k === 0 || k === 2 ? 0.28 : 0.14);
      s += pluck(t, at + k * beat, bass[(b + k) % bass.length], beat * 0.72) * 0.16;
    }
    for (let k = 0; k < 8; k++) {
      s += pluck(t, at + k * beat / 2, motif[(b + k) % motif.length], beat * 0.32) * 0.12;
      if (k % 2 === 1) s += softHat(t, at + k * beat / 2) * 0.05;
    }
  }
  const fade = Math.min(1, t / 0.35, (seconds - t) / 0.7);
  return [s * fade, (s * 0.9 + Math.sin(2 * Math.PI * 0.22 * t) * s * 0.04) * fade];
});

// Quiet: restrained electric-key texture with ample negative space.
await writeWav('quiet.wav', (t) => {
  const beat = 60 / 90;
  const bar = beat * 4;
  const chords = [[48, 55, 60, 64], [45, 52, 57, 60], [41, 48, 53, 57], [43, 50, 55, 59]];
  let s = 0;
  for (let b = 0; b < Math.ceil(seconds / bar); b++) {
    const at = b * bar;
    s += pad(t, at, chords[b % chords.length], bar + 0.6) * 0.22;
    s += pluck(t, at + beat * 0.2, chords[b % chords.length][3] + 12, beat * 0.9) * 0.09;
    s += pluck(t, at + beat * 2.15, chords[b % chords.length][2] + 12, beat * 0.9) * 0.075;
    s += softKick(t, at) * 0.13;
    s += softKick(t, at + beat * 2) * 0.09;
  }
  const fade = Math.min(1, t / 0.8, (seconds - t) / 1.2);
  return [s * fade, s * fade * 0.94];
});

console.log('Generated three original 18-second music beds in public/music/.');
