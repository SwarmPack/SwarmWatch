export {};

const HOST = '127.0.0.1';
const PORT = 4100;
type AgentState =
  | 'inactive'
  | 'idle'
  | 'thinking'
  | 'reading'
  | 'editing'
  | 'running'
  | 'awaiting'
  | 'error'
  | 'done';

type AgentStateEvent = {
  type: 'agent_state';
  agentFamily: 'cursor';
  agentInstanceId: string;
  agentKey: string;
  agentName: 'Cursor';
  state: AgentState;
  detail?: string;
  ts: number;
};

const now = () => Math.floor(Date.now() / 1000);

async function send(state: AgentState, detail?: string) {
  const conversation_id = `c-sim`;
  const event: AgentStateEvent = {
    type: 'agent_state',
    agentFamily: 'cursor',
    agentInstanceId: conversation_id,
    agentKey: `cursor:${conversation_id}`,
    agentName: 'Cursor',
    state,
    detail,
    ts: now()
  };

  const res = await fetch(`http://${HOST}:${PORT}/event`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(event)
  });

  if (!res.ok) {
    const text = await res.text().catch(() => '');
    throw new Error(`Failed to send event: ${res.status} ${text}`);
  }
}

async function sleep(ms: number) {
  return new Promise((r) => setTimeout(r, ms));
}

async function main() {
  console.log('[simulate:cursor] sending events to ws://127.0.0.1:4100');

  await send('idle', 'Waiting for agent activity');
  await sleep(900);

  await send('reading', 'Read src/App.tsx');
  await sleep(900);

  await send('editing', 'Edited src/App.tsx');
  await sleep(900);

  await send('running', 'npm run build');
  await sleep(900);

  await send('running', 'mcp: github.create_issue {title:"test"}');
  await sleep(900);

  await send('awaiting', 'Waiting for your approval (simulated)');
  await sleep(900);

  await send('error', 'Denied: Denied by policy (simulated)');
  await sleep(900);

  await send('error', 'Hook script failed (simulated)');
  await sleep(900);
  await sleep(800);
  await send('done', 'Agent finished');
  console.log('[simulate:cursor] done');
}

main().catch((err) => {
  console.error('[simulate:cursor] error', err);
  process.exitCode = 1;
});
