import { useEffect, useMemo, useRef, useState } from 'react';
import type {
  ActivityItem,
  AgentFamily,
  AgentKey,
  AgentState,
  AgentStateEvent,
  ApprovalRequest,
  SettingsMessage,
  WsMessage
} from './types';

const WS_URL = 'ws://127.0.0.1:4100';

const now = () => Math.floor(Date.now() / 1000);

type AgentStore = {
  byKey: Record<AgentKey, AgentStateEvent>;
  order: AgentKey[]; // most recent first (for all stored keys, not just orbit-visible)
  pendingApprovals: ApprovalRequest[];
  activityByKey: Record<AgentKey, ActivityItem[]>;
  autoApproveFamilies: Record<string, boolean>;
  sendApprovalDecision: (requestId: string, decision: string, reason?: string) => void;
  sendSetAutoApprove: (agentFamily: string, enabled: boolean) => void;
};

function safeParse(text: string): WsMessage | null {
  try {
    const obj = JSON.parse(text) as any;
    if (obj?.type === 'approvals' && Array.isArray(obj.pending)) return obj as WsMessage;
    if (obj?.type === 'settings' && typeof obj.autoApproveFamilies === 'object') return obj as any;
    if (obj?.type !== 'agent_state') return null;
    if (typeof obj.agentFamily !== 'string') return null;
    if (typeof obj.agentInstanceId !== 'string') return null;
    if (typeof obj.agentKey !== 'string' || !obj.agentKey) return null;
    if (typeof obj.agentName !== 'string') return null;
    if (typeof obj.state !== 'string') return null;
    if (typeof obj.ts !== 'number') return null;
    return obj as WsMessage;
  } catch {
    return null;
  }
}

// Keep more sessions in memory than we show in the orbit.
// Orbit visibility is computed in the UI layer.
const MAX_STORE = 64;
const MAX_ACTIVITY = 200;
// Keep in sync with the control plane inactivity timeout.
const INACTIVITY_TIMEOUT_S = 300;

export function useAgentStates(): AgentStore {
  const [store, setStore] = useState<AgentStore>(() => ({
    byKey: {},
    order: [],
    pendingApprovals: [],
    activityByKey: {},
    autoApproveFamilies: {},
    sendApprovalDecision: () => {
      // placeholder; replaced below
    },
    sendSetAutoApprove: () => {
      // placeholder; replaced below
    },
  }));

  const wsRef = useRef<WebSocket | null>(null);

  // Effective view with inactivity applied. We update this only when needed
  // to avoid a whole-tree re-render every second.
  const [effectiveByKey, setEffectiveByKey] = useState<Record<AgentKey, AgentStateEvent>>({});
  const byKeyRef = useRef<Record<AgentKey, AgentStateEvent>>({});

  // IMPORTANT: keep last-seen time based on *receipt time* (local clock), not the event's `ts`.
  // Some clients may send epoch-ms by mistake which would break inactivity calculations.
  const lastSeenRef = useRef<Record<string, number>>({});
  useEffect(() => {
    Object.keys(store.byKey).forEach((k) => {
      if (lastSeenRef.current[k] == null) lastSeenRef.current[k] = now();
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Helper to compute inactivity-applied map with stable object identity where possible.
  function computeEffective(src: Record<AgentKey, AgentStateEvent>): Record<AgentKey, AgentStateEvent> {
    const out: Record<AgentKey, AgentStateEvent> = {};
    const entries = Object.entries(src);
    for (const [k, ev] of entries) {
      const age = now() - (lastSeenRef.current[k] ?? now());
      if (age >= INACTIVITY_TIMEOUT_S) {
        out[k] = { ...ev, state: 'inactive' as AgentState, detail: 'No activity (5m timeout)' };
      } else {
        out[k] = ev;
      }
    }
    return out;
  }

  // Keep refs current for the interval updater.
  useEffect(() => {
    byKeyRef.current = store.byKey;
  }, [store.byKey]);

  useEffect(() => {
    let ws: WebSocket | null = null;
    let stopped = false;

    function connect() {
      if (stopped) return;
      console.log('[ws] connecting', WS_URL);
      ws = new WebSocket(WS_URL);
      wsRef.current = ws;

      ws.onopen = () => {
        console.log('[ws] open', WS_URL);
      };

      ws.onmessage = (msg) => {
        if (typeof msg.data !== 'string') return;
        const parsed = safeParse(msg.data);
        if (!parsed) return;

        if ((parsed as any).type === 'approvals') {
          const ap = (parsed as any).pending as ApprovalRequest[];
          setStore((prev) => ({ ...prev, pendingApprovals: ap }));
          return;
        }

        if ((parsed as any).type === 'settings') {
          const s = parsed as unknown as SettingsMessage;
          setStore((prev) => ({ ...prev, autoApproveFamilies: s.autoApproveFamilies ?? {} }));
          return;
        }

        const ev = parsed as AgentStateEvent;
        const fam: AgentFamily = ev.agentFamily;
        const key: AgentKey = ev.agentKey;

        // Trust server-provided keying. With the updated control plane, key is
        // `${family}:${instanceId}` (one avatar per IDE session/chat).
        const normalized: AgentStateEvent = { ...ev, agentFamily: fam, agentKey: key };
        lastSeenRef.current[key] = now();

        setStore((prev) => {
          const byKey = { ...prev.byKey, [key]: normalized };
          const order = [key, ...prev.order.filter((k) => k !== key)];
          const trimmed = order.slice(0, MAX_STORE);
          const trimmedByKey: Record<string, AgentStateEvent> = {};
          trimmed.forEach((k) => {
            trimmedByKey[k] = byKey[k];
          });

          const nextActivity: ActivityItem = {
            ts: normalized.ts,
            agentInstanceId: normalized.agentInstanceId,
            hook: normalized.hook,
            state: normalized.state,
            detail: normalized.detail,
            projectName: normalized.projectName
          };

          const existing = prev.activityByKey[key] ?? [];
          const merged = [...existing, nextActivity].slice(-MAX_ACTIVITY);
          const activityByKey = { ...prev.activityByKey, [key]: merged };

          // Drop activity for sessions we evicted from the in-memory store.
          // (Orbit eviction is UI-only; store eviction is memory-only.)
          Object.keys(activityByKey).forEach((k) => {
            if (!trimmed.includes(k as AgentKey)) delete (activityByKey as any)[k];
          });

          return { ...prev, byKey: trimmedByKey, order: trimmed, activityByKey };
        });

        // Immediately refresh effective view on inbound events.
        setEffectiveByKey((prev) => {
          const next = computeEffective({ ...store.byKey, [key]: normalized });
          // Shallow compare keys and changed references to avoid needless updates.
          const prevKeys = Object.keys(prev);
          const nextKeys = Object.keys(next);
          if (prevKeys.length !== nextKeys.length) return next;
          for (const k2 of nextKeys) {
            if (prev[k2] === next[k2]) continue;
            // If only state/detail changed to identical strings, skip; otherwise update.
            const a = prev[k2];
            const b = next[k2];
            if (!a || !b) return next;
            if (a.state !== b.state || a.detail !== b.detail) return next;
          }
          return prev;
        });
      };

      ws.onclose = (ev) => {
        console.warn('[ws] close', ev.code, ev.reason);
        if (stopped) return;
        setTimeout(connect, 750);
      };

      ws.onerror = (err) => {
        console.error('[ws] error', err);
        try {
          ws?.close();
        } catch {
          // ignore
        }
      };
    }

    connect();

    return () => {
      stopped = true;
      wsRef.current = null;
      try {
        ws?.close();
      } catch {
        // ignore
      }
    };
  }, []);

  // Periodic inactivity recompute, but only update when something actually changes.
  useEffect(() => {
    const id = window.setInterval(() => {
      const next = computeEffective(byKeyRef.current);
      setEffectiveByKey((prev) => {
        const prevKeys = Object.keys(prev);
        const nextKeys = Object.keys(next);
        if (prevKeys.length !== nextKeys.length) return next;
        for (const k of nextKeys) {
          if (prev[k] === next[k]) continue;
          const a = prev[k];
          const b = next[k];
          if (!a || !b) return next;
          if (a.state !== b.state || a.detail !== b.detail) return next;
        }
        return prev;
      });
    }, 1000);
    return () => window.clearInterval(id);
  }, []);

  // 5m inactivity policy is applied in `effectiveByKey`.
  const derived = useMemo(() => {
    const sendApprovalDecision = (requestId: string, decision: string, reason?: string) => {
      const sock = wsRef.current;
      if (!sock || sock.readyState !== WebSocket.OPEN) {
        console.warn('[ws] cannot send approval decision; socket not open', {
          hasSocket: Boolean(sock),
          readyState: sock?.readyState
        });
        return;
      }
      const payload = { type: 'approval_decision', request_id: requestId, decision, reason };
      console.log('[ws] send approval_decision', payload);
      try {
        // NOTE: control plane expects `request_id` (snake_case), not requestId.
        sock.send(JSON.stringify(payload));
      } catch (err) {
        console.error('[ws] send failed', err);
      }
    };

    const sendSetAutoApprove = (agentFamily: string, enabled: boolean) => {
      const sock = wsRef.current;
      if (!sock || sock.readyState !== WebSocket.OPEN) {
        console.warn('[ws] cannot send set_auto_approve; socket not open', {
          hasSocket: Boolean(sock),
          readyState: sock?.readyState
        });
        return;
      }
      const payload = { type: 'set_auto_approve', agent_family: agentFamily, enabled };
      console.log('[ws] send set_auto_approve', payload);
      try {
        sock.send(JSON.stringify(payload));
      } catch (err) {
        console.error('[ws] send failed', err);
      }
    };

    return { ...store, byKey: effectiveByKey, sendApprovalDecision, sendSetAutoApprove };
  }, [store, effectiveByKey]);

  return derived;
}
