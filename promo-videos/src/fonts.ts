import {loadFont} from '@remotion/fonts';
import {staticFile} from 'remotion';

void Promise.all([
  loadFont({family: 'Inter', url: staticFile('fonts/Inter-SemiBold.ttf'), weight: '600'}),
  loadFont({family: 'Inter', url: staticFile('fonts/Inter-Bold.ttf'), weight: '700'}),
  loadFont({family: 'Inter', url: staticFile('fonts/Inter-ExtraBold.ttf'), weight: '800'}),
]);
