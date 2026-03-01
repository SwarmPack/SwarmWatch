import http from 'node:http';
import { WebSocketServer, WebSocket } from 'ws';

type AgentStateEvent = {
  type: 'agent_state';
  agentFamily: 'cursor' | 'claude' | 'windsurf' | string;
  agentInstanceId: string;
  agentKey: string; // `${agentFamily}:${agentInstanceId}` (normalized)
  agentName: string;
  state:
    | 'inactive'
    | 'idle'
    | 'thinking'
    | 'reading'
    | 'editing'
    | 'running'
    | 'error'
    | 'done'
    | string;
  detail?: string;
  ts: number; // epoch seconds
};

type ServerState = Record<string, AgentStateEvent>; // key = `${agentFamily}:${agentInstanceId}`

type ApprovalRequest = {
  id: string;
  createdAt: number; // epoch seconds
  status: 'pending' | 'approved' | 'denied' | 'expired';
  decision?: 'allow' | 'deny' | 'ask';
  decidedAt?: number;
  reason?: string;
  // metadata for UI
  agentKey: string;
  agentFamily: string;
  agentInstanceId: string;
  hook: string;
  summary: string;
  raw: unknown;
};

const HOST = '127.0.0.1';
const PORT = 4100;

const now = () => Math.floor(Date.now() / 1000);

function uuid(): string {
  // good enough for local request ids
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

function agentKey(ev: Pick<AgentStateEvent, 'agentFamily' | 'agentInstanceId'>): string {
  return `${ev.agentFamily}:${ev.agentInstanceId}`;
}

const state: ServerState = {};

const approvals = new Map<string, ApprovalRequest>();

function seedAgent(family: string, instanceId: string, name: string) {
  const ev: AgentStateEvent = {
    type: 'agent_state',
    agentFamily: family,
    agentInstanceId: instanceId,
    agentKey: `${family}:${instanceId}`,
    agentName: name,
    state: 'inactive',
    detail: 'No events yet',
    ts: now()
  };
  state[agentKey(ev)] = ev;
}

// Seed one “default instance” per family so UI has something to show.
seedAgent('cursor', 'default', 'Cursor');
seedAgent('claude', 'default', 'Claude');
seedAgent('windsurf', 'default', 'Windsurf');

function safeJsonParse(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function normalizeEvent(input: unknown): AgentStateEvent | null {
  if (!input || typeof input !== 'object') return null;
  const obj = input as Record<string, unknown>;
  if (obj.type !== 'agent_state') return null;

  const agentFamily = typeof obj.agentFamily === 'string' ? obj.agentFamily : 'cursor';
  const agentInstanceId = typeof obj.agentInstanceId === 'string' ? obj.agentInstanceId : 'default';
  const normalizedKey = `${agentFamily}:${agentInstanceId}`;
  const agentName = typeof obj.agentName === 'string' ? obj.agentName : 'Agent';
  const st = typeof obj.state === 'string' ? obj.state : null;
  if (!st) return null;
  const detail = typeof obj.detail === 'string' ? obj.detail : undefined;
  const ts = typeof obj.ts === 'number' ? obj.ts : now();

  return {
    type: 'agent_state',
    agentFamily,
    agentInstanceId,
    agentKey: normalizedKey,
    agentName,
    state: st,
    detail,
    ts
  };
}

function normalizeApprovalRequest(input: unknown): Omit<ApprovalRequest, 'id' | 'createdAt' | 'status'> | null {
  if (!input || typeof input !== 'object') return null;
  const obj = input as Record<string, unknown>;
  const agentFamily = typeof obj.agentFamily === 'string' ? obj.agentFamily : 'unknown';
  const agentInstanceId = typeof obj.agentInstanceId === 'string' ? obj.agentInstanceId : 'unknown';
  const hook = typeof obj.hook === 'string' ? obj.hook : 'unknown';
  const summary = typeof obj.summary === 'string' ? obj.summary : hook;
  const raw = obj.raw ?? null;
  const akey = `${agentFamily}:${agentInstanceId}`;
  return {
    agentKey: akey,
    agentFamily,
    agentInstanceId,
    hook,
    summary,
    raw
  };
}

function broadcastApprovals(wss: WebSocketServer) {
  const list = Array.from(approvals.values())
    .filter((a) => a.status === 'pending')
    .sort((a, b) => b.createdAt - a.createdAt);
  broadcast(wss, {
    type: 'approvals',
    pending: list
  } as any);
}

function broadcast(wss: WebSocketServer, event: any) {
  const payload = JSON.stringify(event);
  for (const client of wss.clients) {
    if (client.readyState === WebSocket.OPEN) {
      client.send(payload);
    }
  }
}

const server = http.createServer((req, res) => {
  // NOTE: This server is intentionally tiny. It is the local “control plane” for
  // agent state events. In the future it may also handle bidirectional approvals.
  //
  // V1 behavior: approvals endpoints exist but are disabled (501).

  // A tiny HTTP surface helps local debugging and allows hooks to POST over HTTP if needed.
  if (req.method === 'GET' && req.url === '/health') {
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ ok: true }));
    return;
  }

  if (req.method === 'GET' && req.url === '/state') {
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify(state));
    return;
  }

  if (req.method === 'GET' && req.url === '/approvals') {
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(
      JSON.stringify({
        ok: true,
        pending: Array.from(approvals.values()).filter((a) => a.status === 'pending')
      })
    );
    return;
  }

  if (req.method === 'POST' && req.url === '/event') {
    let body = '';
    req.on('data', (chunk) => (body += String(chunk)));
    req.on('end', () => {
      const parsed = safeJsonParse(body);
      const ev = normalizeEvent(parsed);
      if (!ev) {
        res.writeHead(400, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ ok: false, error: 'Invalid event payload' }));
        return;
      }
      state[agentKey(ev)] = ev;
      broadcast(wss, ev);
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ ok: true }));
    });
    return;
  }

  // --- Approval endpoints (ENABLED) ---
  if (req.method === 'POST' && req.url === '/approval/request') {
    let body = '';
    req.on('data', (chunk) => (body += String(chunk)));
    req.on('end', () => {
      const parsed = safeJsonParse(body);
      const norm = normalizeApprovalRequest(parsed);
      if (!norm) {
        res.writeHead(400, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ ok: false, error: 'Invalid approval request payload' }));
        return;
      }
      const id = uuid();
      const reqObj: ApprovalRequest = {
        id,
        createdAt: now(),
        status: 'pending',
        ...norm
      };
      approvals.set(id, reqObj);
      broadcastApprovals(wss);
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ ok: true, requestId: id }));
    });
    return;
  }

  if (req.method === 'GET' && req.url?.startsWith('/approval/wait/')) {
    const id = req.url.split('/').pop() ?? '';
    const item = approvals.get(id);
    if (!item) {
      res.writeHead(404, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ ok: false, error: 'Unknown requestId' }));
      return;
    }
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(
      JSON.stringify({
        ok: true,
        status: item.status,
        decision: item.decision,
        reason: item.reason,
        decidedAt: item.decidedAt
      })
    );
    return;
  }

  if (req.method === 'POST' && req.url?.startsWith('/approval/decision/')) {
    const id = req.url.split('/').pop() ?? '';
    const item = approvals.get(id);
    if (!item) {
      res.writeHead(404, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ ok: false, error: 'Unknown requestId' }));
      return;
    }
    let body = '';
    req.on('data', (chunk) => (body += String(chunk)));
    req.on('end', () => {
      const parsed = safeJsonParse(body);
      const obj = (parsed ?? {}) as any;
      const decision = obj?.decision;
      if (decision !== 'allow' && decision !== 'deny' && decision !== 'ask') {
        res.writeHead(400, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ ok: false, error: 'decision must be allow|deny|ask' }));
        return;
      }
      item.status = decision === 'deny' ? 'denied' : 'approved';
      item.decision = decision;
      item.reason = typeof obj?.reason === 'string' ? obj.reason : undefined;
      item.decidedAt = now();
      approvals.set(id, item);
      broadcastApprovals(wss);
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ ok: true }));
    });
    return;
  }

  res.writeHead(404);
  res.end();
});

const wss = new WebSocketServer({ server });

// Heartbeat to keep connections healthy.
setInterval(() => {
  for (const client of wss.clients) {
    if (client.readyState === WebSocket.OPEN) {
      try {
        client.ping();
      } catch {
        // ignore
      }
    }
  }
}, 15_000).unref();

wss.on('connection', (ws) => {
  // Immediately send last-known state.
  Object.values(state).forEach((ev) => {
    try {
      ws.send(JSON.stringify(ev));
    } catch {
      // ignore
    }
  });
  broadcastApprovals(wss);
});

server.listen(PORT, HOST, () => {
  console.log(`[avatar-server] ws://%s:%d`, HOST, PORT);
  console.log(`[avatar-server] http://%s:%d/health`, HOST, PORT);
});

server.on('error', (err: any) => {
  if (err?.code === 'EADDRINUSE') {
    console.warn(`[avatar-server] port ${HOST}:${PORT} already in use; assuming server already running`);
    // Exit 0 so `concurrently` doesn't kill the desktop app.
    process.exit(0);
  }
  console.error('[avatar-server] fatal error', err);
  process.exit(1);
});
