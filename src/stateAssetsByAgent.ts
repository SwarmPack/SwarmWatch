import type { AgentState } from './types';

// v1: both agents share the same state→asset mapping.
// If you later want distinct avatars per agent, split these mappings.
export const STATE_TO_LOTTIE_BY_AGENT: Record<string, Record<AgentState, string>> = {
  cursor: {
    inactive: '/base3-idle.json',
    idle: '/base3-idle.json',
    thinking: '/thinking.json',
    reading: '/exp-reading.json',
    editing: '/exp-editing.json',
    running: '/base4-running.json',
    awaiting: '/HelpNeed.json',
    error: '/error.json',
    done: '/done.json'
  },
  claude: {
    inactive: '/base3-idle.json',
    idle: '/base3-idle.json',
    thinking: '/thinking.json',
    reading: '/exp-reading.json',
    editing: '/exp-editing.json',
    running: '/base4-running.json',
    awaiting: '/HelpNeed.json',
    error: '/error.json',
    done: '/done.json'
  },
  windsurf: {
    inactive: '/base3-idle.json',
    idle: '/base3-idle.json',
    thinking: '/thinking.json',
    reading: '/exp-reading.json',
    editing: '/exp-editing.json',
    running: '/base4-running.json',
    awaiting: '/HelpNeed.json',
    error: '/error.json',
    done: '/done.json'
  }
  ,
  vscode: {
    inactive: '/base3-idle.json',
    idle: '/base3-idle.json',
    thinking: '/thinking.json',
    reading: '/exp-reading.json',
    editing: '/exp-editing.json',
    running: '/base4-running.json',
    awaiting: '/HelpNeed.json',
    error: '/error.json',
    done: '/done.json'
  },
  cline: {
    inactive: '/base3-idle.json',
    idle: '/base3-idle.json',
    thinking: '/thinking.json',
    reading: '/exp-reading.json',
    editing: '/exp-editing.json',
    running: '/base4-running.json',
    awaiting: '/HelpNeed.json',
    error: '/error.json',
    done: '/done.json'
  }
};

export function lottieForAgentState(agentFamily: string, state: AgentState): string {
  // Fallback for unknown/new IDE families.
  const map = STATE_TO_LOTTIE_BY_AGENT[agentFamily] ?? STATE_TO_LOTTIE_BY_AGENT.cursor;
  return map[state] ?? STATE_TO_LOTTIE_BY_AGENT.cursor.idle;
}

export type AssetVariant = 'planet' | 'sun' | 'collapsed';

// Return prioritized candidates (first that exists will be used). This allows
// us to prefer a future "-lite" asset for small planets while keeping current
// heavy assets as fallback without breaking existing installs.
export function lottieCandidatesForAgentState(
  agentFamily: string,
  state: AgentState,
  variant: AssetVariant
): string[] {
  const primary = lottieForAgentState(agentFamily, state);
  if (state === 'thinking') {
    if (variant === 'planet' || variant === 'collapsed') {
      // Prefer a light asset if present, else fall back to the current heavy one.
      return ['/thinking-lite.json', primary];
    }
  }
  return [primary];
}
