import type { AgentState } from './types';

// MUST implement exactly as specified in the prompt.
export const STATE_TO_LOTTIE: Record<AgentState, string> = {
  inactive: '/base3-idle.json',
  idle: '/base3-idle.json',
  thinking: '/thinking.json',
  reading: '/exp-reading.json',
  editing: '/exp-editing.json',
  running: '/base4-running.json',
  awaiting: '/HelpNeed.json',
  error: '/error.json',
  done: '/done.json'
};
