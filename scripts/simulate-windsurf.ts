// Simple simulator to send Windsurf-style events to the local control plane.
// Usage: npm run simulate:windsurf

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function post(ev: any) {
  await fetch('http://127.0.0.1:4100/event', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(ev)
  });
}

async function main() {
  const traj = `t-${Date.now()}`;
  const mk = (state: string, detail: string) => ({
    type: 'agent_state',
    agentFamily: 'windsurf',
    agentInstanceId: traj,
    agentKey: `windsurf:${traj}`,
    agentName: 'Windsurf',
    state,
    detail,
    ts: Math.floor(Date.now() / 1000)
  });

  await post(mk('idle', 'Cascade started'));
  await sleep(800);
  await post(mk('reading', 'Reading code'));
  await sleep(800);
  await post(mk('editing', 'Writing code'));
  await sleep(800);
  await post(mk('running', 'Running tests'));
  await sleep(800);
  await post(mk('done', 'Done'));
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
