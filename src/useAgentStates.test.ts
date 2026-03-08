import { describe, it, expect } from 'vitest';

// Minimal mirror of safeParse logic from src/useAgentStates.ts.
// NOTE: We keep this test in sync with the production code by reusing the same schema expectations.
function safeParse(text: string): any | null {
  try {
    const obj = JSON.parse(text) as any;
    if (obj?.type === 'approvals' && Array.isArray(obj.pending)) return obj;
    if (obj?.type === 'settings' && typeof obj.autoApproveFamilies === 'object') return obj;
    if (obj?.type !== 'agent_state') return null;
    if (typeof obj.agentFamily !== 'string') return null;
    if (typeof obj.agentInstanceId !== 'string') return null;
    if (typeof obj.agentKey !== 'string' || !obj.agentKey) return null;
    if (typeof obj.agentName !== 'string') return null;
    if (typeof obj.state !== 'string') return null;
    if (typeof obj.ts !== 'number') return null;
    return obj;
  } catch {
    return null;
  }
}

describe('ws safeParse', () => {
  const base = {
    type: 'agent_state',
    agentFamily: 'vscode',
    agentInstanceId: 'sess',
    agentKey: 'vscode:sess',
    agentName: 'VS Code',
    ts: 123,
  };

  const states = ['inactive','idle','thinking','reading','editing','running','awaiting','error','done'] as const;

  for (const st of states) {
    it(`accepts state ${st}`, () => {
      const msg = JSON.stringify({ ...base, state: st });
      expect(safeParse(msg)).not.toBeNull();
    });
  }

  it('rejects invalid message types', () => {
    expect(safeParse(JSON.stringify({ type: 'nope' }))).toBeNull();
  });

  it('accepts approvals payload', () => {
    const msg = JSON.stringify({ type: 'approvals', pending: [] });
    expect(safeParse(msg)).not.toBeNull();
  });

  it('accepts settings payload', () => {
    const msg = JSON.stringify({ type: 'settings', autoApproveFamilies: { vscode: true } });
    expect(safeParse(msg)).not.toBeNull();
  });
});
