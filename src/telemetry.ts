// Minimal, privacy-preserving telemetry.
//
// Constraints:
// - no prompts, code, file paths, commands, approval payloads, or any user content
// - fire-and-forget: queue + async flush; failures ignored
// - no PostHog autocapture; we only send explicit events

type Platform = 'macos' | 'windows' | 'linux' | 'other';
type UiClickTarget = 'recap' | 'approvals' | 'activity' | 'settings';

type TelemetryContext = {
  distinctId: string;
  platform: Platform;
  appVersion?: string;
};

type PendingEvent = {
  event: string;
  properties: Record<string, any>;
};

const POSTHOG_KEY = (import.meta as any).env?.VITE_POSTHOG_KEY as string | undefined;
const POSTHOG_HOST = (import.meta as any).env?.VITE_POSTHOG_HOST as string | undefined;

// Storage for daily unique session dedupe (local-only).
const DAILY_SESSIONS_KEY = 'swarmwatch.telemetry.dailySessions.v1';
const DAILY_SESSIONS_KEEP_DAYS = 14;

// Queue/flush policy.
// Goal: near-zero overhead while idle + tolerate offline periods.
//
// - We persist the outbound queue to localStorage so we can survive temporary
//   offline periods (up to 2 hours) without dropping events.
// - We use a single debounced flush timer (setTimeout), not a repeating interval.
// - We send using PostHog /batch/ to reduce network requests.
const QUEUE_STORAGE_KEY = 'swarmwatch.telemetry.outbox.v1';
const QUEUE_KEEP_MS = 2 * 60 * 60 * 1000; // 2 hours
const FLUSH_DEBOUNCE_MS = 30_000;
const FLUSH_BATCH_MAX = 50;
const QUEUE_MAX = 2000;

let ctx: TelemetryContext | null = null;
let queue: PendingEvent[] = [];
let flushTimer: number | null = null;
let dailyCache: Record<string, Record<string, Record<string, 1>>> | null = null;

function isTauriRuntime(): boolean {
  const w: any = window as any;
  return Boolean(w?.__TAURI_INTERNALS__ || w?.__TAURI__);
}

function dayLabelLocal(d = new Date()): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, '0');
  const dd = String(d.getDate()).padStart(2, '0');
  return `${y}-${m}-${dd}`;
}

function normalizeHost(host?: string): string | null {
  const raw = String(host ?? '').trim();
  if (!raw) return null;
  // Allow users to set either https://... or a bare domain.
  const withProto = raw.startsWith('http://') || raw.startsWith('https://') ? raw : `https://${raw}`;
  return withProto.replace(/\/+$/, '');
}

function captureUrl(): string | null {
  const host = normalizeHost(POSTHOG_HOST);
  if (!host) return null;
  return `${host}/capture/`;
}

function batchUrl(): string | null {
  const host = normalizeHost(POSTHOG_HOST);
  if (!host) return null;
  return `${host}/batch/`;
}

function telemetryEnabled(): boolean {
  return Boolean(POSTHOG_KEY && normalizeHost(POSTHOG_HOST) && ctx?.distinctId);
}

function nowMs(): number {
  return Date.now();
}

function saveQueueBestEffort() {
  try {
    const payload = {
      v: 1,
      savedAtMs: nowMs(),
      queue
    };
    localStorage.setItem(QUEUE_STORAGE_KEY, JSON.stringify(payload));
  } catch {
    // ignore
  }
}

function loadQueueBestEffort() {
  try {
    const raw = localStorage.getItem(QUEUE_STORAGE_KEY);
    if (!raw) return;
    const parsed = JSON.parse(raw);
    const savedAtMs = Number(parsed?.savedAtMs ?? 0);
    if (!savedAtMs || nowMs() - savedAtMs > QUEUE_KEEP_MS) {
      localStorage.removeItem(QUEUE_STORAGE_KEY);
      return;
    }
    const q = parsed?.queue;
    if (!Array.isArray(q)) return;
    // Minimal validation: ensure shape.
    const restored: PendingEvent[] = [];
    for (const it of q) {
      if (!it || typeof it !== 'object') continue;
      if (typeof it.event !== 'string') continue;
      if (!it.properties || typeof it.properties !== 'object') continue;
      restored.push({ event: it.event, properties: it.properties });
    }
    if (restored.length) queue = restored.slice(-QUEUE_MAX);
  } catch {
    // ignore
  }
}

function enqueue(ev: PendingEvent) {
  // Never let telemetry grow without bound.
  if (queue.length >= QUEUE_MAX) {
    queue = queue.slice(queue.length - Math.floor(QUEUE_MAX / 2));
  }
  queue.push(ev);

  // Persist so we can survive temporary offline.
  saveQueueBestEffort();

  // Debounced flush.
  scheduleFlush();
}

function trySendBeacon(url: string, body: string): boolean {
  try {
    if (typeof navigator?.sendBeacon !== 'function') return false;
    const blob = new Blob([body], { type: 'application/json' });
    return navigator.sendBeacon(url, blob);
  } catch {
    return false;
  }
}

async function tryFetch(url: string, body: string): Promise<void> {
  try {
    const ctrl = new AbortController();
    const t = window.setTimeout(() => ctrl.abort(), 1_000);
    await fetch(url, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body,
      // keepalive makes the request best-effort during page unload.
      keepalive: true,
      signal: ctrl.signal
    });
    window.clearTimeout(t);
  } catch {
    // ignore
  }
}

async function flushOnce(): Promise<void> {
  if (!telemetryEnabled()) return;
  const url = batchUrl();
  if (!url) return;
  if (!queue.length) return;

  const batch = queue.splice(0, FLUSH_BATCH_MAX);
  // Save queue immediately after dequeue so we don't resend on crash.
  saveQueueBestEffort();

  const payload = {
    api_key: POSTHOG_KEY,
    batch: batch.map((it) => ({
      event: it.event,
      // PostHog expects distinct_id inside properties.
      properties: {
        distinct_id: ctx?.distinctId,
        platform: ctx?.platform,
        app_version: ctx?.appVersion,
        ...it.properties
      }
    }))
  };

  const body = JSON.stringify(payload);
  const ok = trySendBeacon(url, body);
  if (!ok) {
    void Promise.resolve().then(() => tryFetch(url, body));
  }
}

function scheduleFlush() {
  // Debounce: reset timer on each enqueue.
  if (flushTimer != null) {
    window.clearTimeout(flushTimer);
    flushTimer = null;
  }
  flushTimer = window.setTimeout(() => {
    flushTimer = null;
    void flushOnce();
  }, FLUSH_DEBOUNCE_MS);
}

function loadDailyCache(): Record<string, Record<string, Record<string, 1>>> {
  if (dailyCache) return dailyCache;
  try {
    const raw = localStorage.getItem(DAILY_SESSIONS_KEY);
    if (!raw) {
      dailyCache = {};
      return dailyCache;
    }
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') {
      dailyCache = {};
      return dailyCache;
    }
    dailyCache = parsed;
    return dailyCache;
  } catch {
    dailyCache = {};
    return dailyCache;
  }
}

function saveDailyCacheBestEffort() {
  if (!dailyCache) return;
  try {
    localStorage.setItem(DAILY_SESSIONS_KEY, JSON.stringify(dailyCache));
  } catch {
    // ignore
  }
}

function pruneDailyCacheBestEffort() {
  const cache = loadDailyCache();
  const cutoff = new Date();
  cutoff.setDate(cutoff.getDate() - DAILY_SESSIONS_KEEP_DAYS);

  const keys = Object.keys(cache);
  for (const day of keys) {
    // day format YYYY-MM-DD (local). Parse safely.
    const m = day.match(/^(\d{4})-(\d{2})-(\d{2})$/);
    if (!m) {
      delete cache[day];
      continue;
    }
    const y = Number(m[1]);
    const mon = Number(m[2]) - 1;
    const d = Number(m[3]);
    const dt = new Date(y, mon, d);
    if (Number.isNaN(dt.getTime())) {
      delete cache[day];
      continue;
    }
    if (dt < cutoff) delete cache[day];
  }
  saveDailyCacheBestEffort();
}

function allowlistedFamily(family: string): string | null {
  const f = String(family ?? '').trim().toLowerCase();
  if (f === 'cursor' || f === 'claude' || f === 'cline' || f === 'vscode') return f;
  return null;
}

// Public API

export async function initTelemetry(): Promise<void> {
  // If config is missing, keep telemetry inert.
  if (!POSTHOG_KEY || !normalizeHost(POSTHOG_HOST)) return;
  if (!isTauriRuntime()) return;

  try {
    const [{ invoke }, { getVersion }] = await Promise.all([
      import('@tauri-apps/api/core'),
      import('@tauri-apps/api/app')
    ]);

    const raw = (await invoke('telemetry_context')) as any;
    const distinctId = String(raw?.distinctId ?? '').trim();
    const platform = String(raw?.platform ?? '').trim() as Platform;
    if (!distinctId) return;

    let appVersion: string | undefined;
    try {
      appVersion = await getVersion();
    } catch {
      // ignore
    }

    ctx = {
      distinctId,
      platform: (platform || 'other') as Platform,
      appVersion
    };

    // Restore any queued events (best-effort), but only if recent.
    loadQueueBestEffort();

    // Maintenance tasks.
    pruneDailyCacheBestEffort();
    // Flush is scheduled lazily on enqueue, but if we restored a queue,
    // schedule a flush soon.
    if (queue.length) scheduleFlush();

    // Try to flush on visibility change too (best-effort).
    document.addEventListener('visibilitychange', () => {
      if (!document.hidden) void flushOnce();
    });

    // Flush on process exit best-effort.
    window.addEventListener('beforeunload', () => {
      void flushOnce();
    });
  } catch {
    // ignore
  }
}

export function trackUiClick(target: UiClickTarget, state?: 'open' | 'close') {
  if (!telemetryEnabled()) return;
  enqueue({
    event: 'swarmwatch_ui_click',
    properties: {
      target,
      ...(state ? { state } : {})
    }
  });
}

// Called when we receive any agent_state event (receipt-time semantics).
// Dedupes by (local day, family, agentKey) locally, but does NOT send agentKey.
export function trackAvatarSessionDaily(ev: { agentFamily?: string; agentKey?: string }) {
  if (!telemetryEnabled()) return;
  const fam = allowlistedFamily(String(ev?.agentFamily ?? ''));
  if (!fam) return;
  const agentKey = String(ev?.agentKey ?? '').trim();
  if (!agentKey) return;

  const day = dayLabelLocal();
  const cache = loadDailyCache();
  cache[day] = cache[day] || {};
  cache[day][fam] = cache[day][fam] || {};
  if (cache[day][fam][agentKey]) return;

  cache[day][fam][agentKey] = 1;
  saveDailyCacheBestEffort();

  enqueue({
    event: 'swarmwatch_avatar_session_daily',
    properties: {
      day,
      agent_family: fam
    }
  });
}

// Debug/testing helper (dev builds only): allows validating that capture is
// correctly configured without needing live agent events.
export function _debugTelemetryPing() {
  if (!telemetryEnabled()) return;
  enqueue({
    event: 'swarmwatch_telemetry_ping',
    properties: {}
  });
}
