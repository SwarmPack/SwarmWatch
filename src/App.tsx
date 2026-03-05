import { useEffect, useMemo, useRef, useState } from 'react';
import { format } from 'date-fns';
import './App.css';
import { useAgentStates } from './useAgentStates';
import type { AgentKey } from './types';
import { lottieCandidatesForAgentState } from './stateAssetsByAgent';
import { LottieCircle } from './components/LottieCircle';

type UpdateStatus =
  | { state: 'idle' }
  | { state: 'checking' }
  | { state: 'available'; version?: string }
  | { state: 'downloading'; progress?: number }
  | { state: 'ready' }
  | { state: 'error'; message: string };

const INTEGRATION_ITEMS = [
  { key: 'cursor', label: 'Cursor' },
  { key: 'claude', label: 'Claude Code' },
  { key: 'vscode', label: 'VS Code' },
  // Windsurf temporarily hidden from UI while unsupported.
  // { key: 'windsurf', label: 'Windsurf' },
  { key: 'cline', label: 'Cline' }
] as const;

type IntegrationKey = (typeof INTEGRATION_ITEMS)[number]['key'];

function isIntegrationKey(x: string): x is IntegrationKey {
	return (INTEGRATION_ITEMS as readonly { key: string }[]).some((it) => it.key === x);
}

type IntegrationsStatus = {
  runner?: { path?: string; exists?: boolean };
  cursor: IntegrationRow;
  claude: IntegrationRow;
  // VS Code Copilot Agent hooks (preview). Workspace-only hooks.
  vscode: IntegrationRow;
  windsurf: IntegrationRow;
  cline: IntegrationRow;
};

type IntegrationRow = {
  supported: boolean;
  detected: boolean;
  enabled: boolean;
  // Optional extra diagnostics from backend (safe for older builds).
  familyEnabled?: boolean;
  configEnabled?: boolean;
  hooksEnabled?: boolean | null;
  hookFilesLocationsEnabled?: boolean | null;
  hooksSettingsPath?: string | null;
  shim?: { path?: string; exists?: boolean };
  // VS Code / future Cline: optional per-workspace diagnostics from backend.
  workspaces?: any[];
};

type AutoApproveFamily = 'cursor' | 'claude' | 'vscode' | 'cline' | 'windsurf';

type WindowApi = typeof import('@tauri-apps/api/window');

function isTauriRuntime(): boolean {
  const w: any = window as any;
  // Tauri v2 does NOT require `withGlobalTauri` so `window.__TAURI__` may be absent.
  // The internal bridge is still present.
  return Boolean(w?.__TAURI_INTERNALS__ || w?.__TAURI__);
}

async function tryLoadWindowApi(): Promise<WindowApi | null> {
  try {
    // Keep browser preview working by only loading the API when actually
    // running inside Tauri.
    if (!isTauriRuntime()) return null;
    return (await import('@tauri-apps/api/window')) as WindowApi;
  } catch {
    return null;
  }
}

function App() {
  const {
    byKey,
    order,
    activityByKey,
    pendingApprovals,
    sendApprovalDecision,
    sendSetAutoApprove,
    autoApproveFamilies
  } = useAgentStates();
  const [expanded, setExpanded] = useState(false);
  const [selectedAgentKey, setSelectedAgentKey] = useState<AgentKey>('system:idle');
  const AGENT_NAMES_KEY = 'swarmwatch.overlay.agentNames.v1';
  const [customAgentNames, setCustomAgentNames] = useState<Record<AgentKey, string>>(() => {
    try {
      const raw = localStorage.getItem(AGENT_NAMES_KEY);
      if (!raw) return {};
      const parsed = JSON.parse(raw);
      return typeof parsed === 'object' && parsed ? parsed : {};
    } catch {
      return {};
    }
  });

  function setAgentDisplayName(agentKey: AgentKey, name: string) {
    setCustomAgentNames((prev) => {
      const next = { ...prev, [agentKey]: name };
      try {
        localStorage.setItem(AGENT_NAMES_KEY, JSON.stringify(next));
      } catch {
        // ignore
      }
      return next;
    });
  }

  const expandedRef = useRef(expanded);
  expandedRef.current = expanded;

  const POS_KEY = 'swarmwatch.overlay.bubblePos.v2';
  const COLLAPSED_SIZE = 84;
  const EXPANDED_SIZE = 420;
  const POS_SANITY_KEY = 'swarmwatch.overlay.posSanity.v2';

  const windowApiRef = useRef<WindowApi | null>(null);
  const scaleFactorRef = useRef<number>(1);
  const lastCollapsedPosRef = useRef<{ x: number; y: number } | null>(null);
  const collapsedClickThroughRef = useRef<boolean>(false);

  const DRAG_THRESHOLD_PX = 18;

  // Drag move coalescing: pointermove can fire very fast; if we `await setPosition`
  // per event, we get many concurrent async calls which complete out-of-order.
  // That feels like flicker/jumping. So we coalesce to at most 1 window move per frame.
  const dragPendingPosRef = useRef<{ x: number; y: number } | null>(null);
  const dragSetPosInFlightRef = useRef<boolean>(false);
  const expandFallbackTimerRef = useRef<number | null>(null);
  const dragLoopScheduledRef = useRef<boolean>(false);

  // Pointer-based drag: only start OS-level dragging once the pointer moved
  // a small threshold, so a normal click still toggles expanded mode.
  const pointerRef = useRef<{
    isDown: boolean;
    didDrag: boolean;
    startX: number;
    startY: number;
  }>({ isDown: false, didDrag: false, startX: 0, startY: 0 });

  const selectedAgent = useMemo(() => {
    const a = byKey[selectedAgentKey] ?? (order[0] ? byKey[order[0]] : undefined);
    if (a) return a;
    // UI placeholder when there are no real sessions.
    return {
      type: 'agent_state' as const,
      agentFamily: 'system',
      agentInstanceId: 'idle',
      agentKey: 'system:idle',
      agentName: 'No active sessions',
      state: 'idle' as const,
      detail: 'Waiting for hook events…',
      ts: Math.floor(Date.now() / 1000)
    };
  }, [byKey, order, selectedAgentKey]);

  const selectedAgentName = customAgentNames[selectedAgentKey] || selectedAgent.agentName;

  function sanitizeProjectName(projectName?: string, detail?: string): string | null {
    if (projectName) {
      // If a buggy adapter sends a path, convert to basename.
      const raw = projectName.trim();
      const cleaned = raw.replace(/^cd\s+/i, '').trim();
      const parts = cleaned.split(/[\\/]/).filter(Boolean);
      return parts.length ? parts[parts.length - 1] : cleaned;
    }

    // Fallback: try to infer from detail like `cd /a/b/c && npm test`.
    if (detail) {
      const m = detail.match(/\bcd\s+([^&;]+?)(?:\s*&&|\s*$)/i);
      if (m?.[1]) {
        const parts = m[1].trim().split(/[\\/]/).filter(Boolean);
        if (parts.length) return parts[parts.length - 1];
      }
    }
    return null;
  }

  const selectedProjectName = sanitizeProjectName(selectedAgent.projectName, selectedAgent.detail);
  const collapsedCandidates = useMemo(
    () => lottieCandidatesForAgentState(selectedAgent.agentFamily, selectedAgent.state, 'collapsed'),
    [selectedAgent.agentFamily, selectedAgent.state]
  );
  const collapsedLoop = selectedAgent.state !== 'done';
  // Performance: when there are no active sessions (system:idle), avoid animating
  // the collapsed avatar to keep CPU near-zero while idle.
  const collapsedPlaying = selectedAgent.agentKey !== 'system:idle';

  const [integrations, setIntegrations] = useState<IntegrationsStatus | null>(null);
  const [integrationsErr, setIntegrationsErr] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [activityOpen, setActivityOpen] = useState(false);
  const [approvalsOpen, setApprovalsOpen] = useState(false);

  // ---------------- Updater banner (Tauri) ----------------
  const UPDATE_DISMISS_KEY = 'swarmwatch.updater.bannerDismissed.v1';
  const [updateDismissed, setUpdateDismissed] = useState<boolean>(() => {
    try {
      return localStorage.getItem(UPDATE_DISMISS_KEY) === '1';
    } catch {
      return false;
    }
  });
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>({ state: 'idle' });
  const updateCheckedRef = useRef(false);
  const [appVersion, setAppVersion] = useState<string | null>(null);

  async function checkForUpdatesOnce() {
    if (updateCheckedRef.current) return;
    updateCheckedRef.current = true;

    if (!isTauriRuntime()) return;
    try {
      setUpdateStatus({ state: 'checking' });

      // Tauri updater v2: check() returns Update | null.
      const { check } = await import('@tauri-apps/plugin-updater');
      const upd = await check();
      if (upd) {
        setUpdateStatus({ state: 'available', version: upd.version });
      } else {
        setUpdateStatus({ state: 'idle' });
      }
    } catch (e: any) {
      setUpdateStatus({ state: 'error', message: String(e?.message ?? e) });
    }
  }

  async function loadAppVersionOnce() {
    if (!isTauriRuntime()) return;
    try {
      const { getVersion } = await import('@tauri-apps/api/app');
      const v = await getVersion();
      setAppVersion(v);
    } catch {
      // ignore (web preview)
    }
  }

  async function downloadAndInstallUpdate() {
    if (!isTauriRuntime()) return;
    try {
      const { check } = await import('@tauri-apps/plugin-updater');
      const upd = await check();
      if (!upd) {
        setUpdateStatus({ state: 'idle' });
        return;
      }

      setUpdateStatus({ state: 'downloading' });
      await upd.downloadAndInstall((ev: any) => {
        // Plugin emits DownloadEvent: Started|Progress|Finished.
        if (ev?.event === 'Progress' && typeof ev?.data?.chunkLength === 'number') {
          // We don't know total size reliably here, so just show indeterminate.
          setUpdateStatus({ state: 'downloading' });
        }
      });

      setUpdateStatus({ state: 'ready' });

      // Ensure the new version is actually running.
      // Use a Rust command to avoid JS API/plugin mismatches.
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('app_restart');
      } catch {
        // Best-effort: user can manually relaunch.
      }
    } catch (e: any) {
      setUpdateStatus({ state: 'error', message: String(e?.message ?? e) });
    }
  }

  const topApproval = useMemo(() => {
    const arr = [...(pendingApprovals ?? [])];
    // Prefer approvals for the currently selected agentKey.
    const forSelected = arr.filter((a) => a.agentKey === selectedAgent.agentKey);
    const cand = (forSelected.length ? forSelected : arr).sort((a, b) => (b.createdAt ?? 0) - (a.createdAt ?? 0));
    return cand[0] ?? null;
  }, [pendingApprovals, selectedAgent.agentKey]);

  function postApprovalDecision(id: string, decision: string) {
    sendApprovalDecision(id, decision);
  }

  function setAutoApproveForFamily(family: string, enabled: boolean) {
    sendSetAutoApprove(family, enabled);
  }

  // Removed: "Open in IDE/Terminal" action for approvals (unnecessary UI clutter).

  async function refreshIntegrations() {
    try {
      setIntegrationsErr(null);
      if (!isTauriRuntime()) {
        setIntegrations(null);
        return;
      }
      const { invoke } = await import('@tauri-apps/api/core');
      const res = (await invoke('integrations_status')) as IntegrationsStatus;
      setIntegrations(res);
    } catch (e: any) {
      setIntegrationsErr(String(e?.message ?? e));
    }
  }

  async function enableIntegration(target: 'cursor' | 'claude' | 'windsurf' | 'vscode' | 'cline') {
    try {
      setIntegrationsErr(null);
      if (!isTauriRuntime()) return;
      const { invoke } = await import('@tauri-apps/api/core');
      // VS Code is workspace-based; the main Enable button just opens
      // the workspace panel instead of calling a global enable.
      if (target === 'vscode') {
        setVscodePanelOpen(true);
        await refreshIntegrations();
        return;
      }
      if (target === 'cline') {
        setClinePanelOpen(true);
        await refreshIntegrations();
        return;
      }
      await invoke('integrations_enable', { target });
      await refreshIntegrations();
    } catch (e: any) {
      setIntegrationsErr(String(e?.message ?? e));
    }
  }

  async function disableIntegration(target: 'cursor' | 'claude' | 'windsurf' | 'vscode' | 'cline') {
    try {
      setIntegrationsErr(null);
      if (!isTauriRuntime()) return;
      const { invoke } = await import('@tauri-apps/api/core');
      // VS Code disable is handled at the workspace level; the row's
      // Disable button triggers a helper that removes all workspaces.
      if (target === 'vscode') {
        // No-op here; UI uses disableAllVscodeWorkspaces instead.
        return;
      }
      if (target === 'cline') {
        // No-op here; UI uses disableAllClineWorkspaces instead.
        return;
      }
      await invoke('integrations_disable', { target });
      await refreshIntegrations();
    } catch (e: any) {
      setIntegrationsErr(String(e?.message ?? e));
    }
  }

  // VS Code (workspace) helpers – workspace list + Browse
  const [vscodePanelOpen, setVscodePanelOpen] = useState(false);
  const [clinePanelOpen, setClinePanelOpen] = useState(false);

  async function browseVscodeWorkspace() {
    try {
      setIntegrationsErr(null);
      if (!isTauriRuntime()) return;
      const [{ open }, { invoke }] = await Promise.all([
        import('@tauri-apps/plugin-dialog'),
        import('@tauri-apps/api/core')
      ]);
      const selection = await open({ directory: true, multiple: false });
      const path = typeof selection === 'string' ? selection : null;
      if (!path) return;
      await invoke('integrations_vscode_enable_for_workspace', { workspacePath: path });
      await refreshIntegrations();
    } catch (e: any) {
      setIntegrationsErr(String(e?.message ?? e));
    }
  }

  async function browseClineWorkspace() {
    try {
      setIntegrationsErr(null);
      if (!isTauriRuntime()) return;
      const [{ open }, { invoke }] = await Promise.all([
        import('@tauri-apps/plugin-dialog'),
        import('@tauri-apps/api/core')
      ]);
      const selection = await open({ directory: true, multiple: false });
      const path = typeof selection === 'string' ? selection : null;
      if (!path) return;
      await invoke('integrations_cline_enable_for_workspace', { workspacePath: path });
      await refreshIntegrations();
    } catch (e: any) {
      setIntegrationsErr(String(e?.message ?? e));
    }
  }

  async function disableAllVscodeWorkspaces() {
    try {
      setIntegrationsErr(null);
      if (!isTauriRuntime()) return;
      const { invoke } = await import('@tauri-apps/api/core');
      const ws = (integrations?.vscode as any)?.workspaces as any[] | undefined;
      if (!ws || !ws.length) return;
      for (const s of ws) {
        const p = s?.workspacePath as string | undefined;
        if (!p) continue;
        // Best-effort: ignore individual errors, refresh at the end.
        try {
          // eslint-disable-next-line no-await-in-loop
          await invoke('integrations_vscode_disable_for_workspace', { workspacePath: p });
        } catch {
          // ignore
        }
      }
      await refreshIntegrations();
      setVscodePanelOpen(false);
    } catch (e: any) {
      setIntegrationsErr(String(e?.message ?? e));
    }
  }

  async function disableAllClineWorkspaces() {
    try {
      setIntegrationsErr(null);
      if (!isTauriRuntime()) return;
      const { invoke } = await import('@tauri-apps/api/core');
      const ws = (integrations?.cline as any)?.workspaces as any[] | undefined;
      if (!ws || !ws.length) return;
      for (const s of ws) {
        const p = s?.workspacePath as string | undefined;
        if (!p) continue;
        try {
          // eslint-disable-next-line no-await-in-loop
          await invoke('integrations_cline_disable_for_workspace', { workspacePath: p });
        } catch {
          // ignore
        }
      }
      await refreshIntegrations();
      setClinePanelOpen(false);
    } catch (e: any) {
      setIntegrationsErr(String(e?.message ?? e));
    }
  }

  async function disableVscodeWorkspacePath(path: string) {
    try {
      setIntegrationsErr(null);
      if (!isTauriRuntime()) return;
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('integrations_vscode_disable_for_workspace', { workspacePath: path });
      await refreshIntegrations();
    } catch (e: any) {
      setIntegrationsErr(String(e?.message ?? e));
    }
  }

  async function disableClineWorkspacePath(path: string) {
    try {
      setIntegrationsErr(null);
      if (!isTauriRuntime()) return;
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('integrations_cline_disable_for_workspace', { workspacePath: path });
      await refreshIntegrations();
    } catch (e: any) {
      setIntegrationsErr(String(e?.message ?? e));
    }
  }

  // Load integrations status once so we can decide which avatars to show.
  useEffect(() => {
    void refreshIntegrations();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Check for updates once when the main UI is expanded.
  useEffect(() => {
    if (!expanded) return;
    if (updateDismissed) return;
    void loadAppVersionOnce();
    void checkForUpdatesOnce();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [expanded, updateDismissed]);

  // Open the VS Code workspace panel by default when VS Code is enabled
  // or when there are any saved workspaces, so the user always sees the
  // repos and Add repo control without having to discover the arrow first.
  useEffect(() => {
    if (!integrations) return;
    const vs = integrations.vscode as any;
    const hasWorkspaces = Array.isArray(vs?.workspaces) && vs.workspaces.length > 0;
    if (!vscodePanelOpen && (vs?.enabled || hasWorkspaces)) {
      setVscodePanelOpen(true);
    }
  }, [integrations, vscodePanelOpen]);

  useEffect(() => {
    if (!integrations) return;
    const cl = integrations.cline as any;
    const hasWorkspaces = Array.isArray(cl?.workspaces) && cl.workspaces.length > 0;
    if (!clinePanelOpen && (cl?.enabled || hasWorkspaces)) {
      setClinePanelOpen(true);
    }
  }, [integrations, clinePanelOpen]);

  const enabledFamilies = useMemo(() => {
    if (!integrations) return null;
    const fams: Array<'cursor' | 'claude' | 'windsurf' | 'vscode' | 'cline'> = [];
    if (integrations.cursor?.enabled) fams.push('cursor');
    if (integrations.claude?.enabled) fams.push('claude');
    if (integrations.windsurf?.enabled) fams.push('windsurf');
    if (integrations.vscode?.enabled) fams.push('vscode');
    if (integrations.cline?.enabled) fams.push('cline');
    return fams.length ? fams : null;
  }, [integrations]);

  const [toast, setToast] = useState<string | null>(null);

  const AUTO_APPROVE_FAMILIES: AutoApproveFamily[] = ['cursor', 'claude', 'vscode', 'cline', 'windsurf'];

  const enabledFamiliesForApprovals = useMemo(() => {
    // Only show IDEs that are enabled in Integrations.
    const enabled = enabledFamilies ?? [];
    const enabledSet = new Set(enabled);
    return AUTO_APPROVE_FAMILIES.filter((f) => enabledSet.has(f as any));
  }, [enabledFamilies]);

  // Orbit rotation angle (CSS variable). We drive this via rAF *without* React
  // re-renders so the UI stays stable and we avoid sync drift.
  const orbitSpinRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    let raf = 0;
    const start = performance.now();

    const tickRot = () => {
      const ms = performance.now() - start;
      // Match prior CSS: 56s per full revolution.
      const deg = ((ms / 56_000) * 360) % 360;
      orbitSpinRef.current?.style.setProperty('--orbit-rot', `${deg}deg`);
      raf = window.requestAnimationFrame(tickRot);
    };

    const startRaf = () => {
      if (raf) return;
      raf = window.requestAnimationFrame(tickRot);
    };
    const stopRaf = () => {
      if (!raf) return;
      window.cancelAnimationFrame(raf);
      raf = 0;
    };

    const onVis = () => {
      // Only spin the orbit when expanded and visible.
      if (document.hidden || !expandedRef.current) stopRaf();
      else startRaf();
    };

    document.addEventListener('visibilitychange', onVis);
    // Initial start: only when expanded.
    if (!document.hidden && expandedRef.current) startRaf();

    return () => {
      document.removeEventListener('visibilitychange', onVis);
      stopRaf();
    };
  }, []);

  // Start/stop orbit animation when expanded toggles.
  useEffect(() => {
    // Trigger the visibility handler logic from the effect above.
    // This keeps the orbit rAF fully off while collapsed.
    const ev = new Event('visibilitychange');
    document.dispatchEvent(ev);
  }, [expanded]);

  // Per-session dismissal for inactive avatars (not persisted).
  const [dismissedInactiveKeys, setDismissedInactiveKeys] = useState<Set<AgentKey>>(() => new Set());

  function dismissInactiveAvatar(k: AgentKey) {
    setDismissedInactiveKeys((prev) => {
      const next = new Set(prev);
      next.add(k);
      return next;
    });
  }

  // Orbit policy:
  // - max 8 avatars
  // - always keep pending approvals visible
  // - fill remaining slots by recency (`order`)
  const orbitVisible = useMemo(() => {
    const orbitMax = 8;
    const enabledSet = enabledFamilies ? new Set(enabledFamilies) : null;

    const pendingKeys = (pendingApprovals ?? [])
      .map((a) => a.agentKey)
      .filter((k) => Boolean(byKey[k])) as AgentKey[];
    const uniquePending = Array.from(new Set(pendingKeys));

    const recency = (order ?? []).filter((k) => Boolean(byKey[k]));
    const merged = [...uniquePending, ...recency.filter((k) => !uniquePending.includes(k))];

    // Hide dismissed inactive avatars (per-session).
    const dismissed = dismissedInactiveKeys;
    const withoutDismissed = merged.filter((k) => {
      const st = byKey[k]?.state;
      return !(dismissed.has(k) && st === 'inactive');
    });

    const filtered = enabledSet
	  ? withoutDismissed.filter((k) => {
			const fam = String(byKey[k]?.agentFamily ?? '').toLowerCase();
			return isIntegrationKey(fam) ? enabledSet.has(fam) : false;
		})
      : withoutDismissed;

    return filtered.slice(0, orbitMax);
  }, [pendingApprovals, byKey, order, enabledFamilies, dismissedInactiveKeys]);

  // Overflow = known sessions not currently in orbit.
  const overflowKeys = useMemo(() => {
    const visible = new Set(orbitVisible);
    return (order ?? []).filter((k) => {
      if (!Boolean(byKey[k]) || visible.has(k)) return false;
      if (dismissedInactiveKeys.has(k) && byKey[k]?.state === 'inactive') return false;
      return true;
    });
  }, [order, byKey, orbitVisible, dismissedInactiveKeys]);

  // Toast when orbit evicts a session (best-effort).
  const prevOrbitRef = useRef<string>('');
  useEffect(() => {
    const prev = prevOrbitRef.current ? prevOrbitRef.current.split('|').filter(Boolean) : [];
    const next = orbitVisible;
    prevOrbitRef.current = next.join('|');
    if (!prev.length) return;
    const dropped = prev.filter((k) => !next.includes(k));
    if (!dropped.length) return;
    // Only toast for the first dropped key to avoid spam.
    const k = dropped[0];
    const ev = byKey[k];
    if (!ev) return;
    setToast(`Orbit full (8): hiding ${ev.agentName} (${ev.agentInstanceId})`);
    const t = window.setTimeout(() => setToast(null), 3500);
    return () => window.clearTimeout(t);
  }, [orbitVisible.join('|')]);

  // If the selected avatar is not visible, auto-select a visible avatar.
  useEffect(() => {
    if (!orbitVisible.length) return;
    if (orbitVisible.includes(selectedAgentKey)) return;
    setSelectedAgentKey(orbitVisible[0]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [orbitVisible.join('|')]);

  // Close the expanded view automatically when focus is lost (user clicks outside the app).
  useEffect(() => {
    if (!expanded) return;
    const onBlur = () => setExpanded(false);
    window.addEventListener('blur', onBlur);
    return () => window.removeEventListener('blur', onBlur);
  }, [expanded]);

  useEffect(() => {
    let cancelled = false;

    (async () => {
      const api = await tryLoadWindowApi();
      if (!api || cancelled) return;
      windowApiRef.current = api;

      const win = api.getCurrentWindow();
      await win.setResizable(false);
      await win.setAlwaysOnTop(true);

      // IMPORTANT: Tauri window positions are in physical pixels.
      // Keep a scaleFactor so we don't mix logical sizes with physical positions.
      scaleFactorRef.current = await win.scaleFactor();

      // If the window was dragged way off-screen (common with multi-monitor changes),
      // reset to a safe default at least once.
      try {
        const flagged = localStorage.getItem(POS_SANITY_KEY);
        if (!flagged) {
          localStorage.setItem(POS_SANITY_KEY, '1');
          lastCollapsedPosRef.current = null;
          localStorage.removeItem(POS_KEY);
        }
      } catch {
        // ignore
      }

      // Restore last collapsed position if we have one.
      try {
        const saved = localStorage.getItem(POS_KEY);
        if (saved) {
          const parsed = JSON.parse(saved);
          if (typeof parsed?.x === 'number' && typeof parsed?.y === 'number') {
            lastCollapsedPosRef.current = { x: parsed.x, y: parsed.y };
          }
        }
      } catch {
        // ignore
      }

      // On first run (no saved pos), place it near top-right.
      if (!lastCollapsedPosRef.current) {
        const monitor = await api.currentMonitor();
        const monitorSize = monitor?.size;
        if (monitorSize) {
          const sf = scaleFactorRef.current || 1;
          const paddingPx = Math.round(16 * sf);
          const collapsedPx = Math.round(COLLAPSED_SIZE * sf);
          const x = Math.round(monitorSize.width - collapsedPx - paddingPx);
          const y = Math.round(paddingPx + Math.round(48 * sf));
          lastCollapsedPosRef.current = { x, y };
          localStorage.setItem(POS_KEY, JSON.stringify({ x, y }));
        }
      }

      // Persist the window position while collapsed (when the user drags it).
      const unlisten = await win.onMoved((e: any) => {
        if (expandedRef.current) return;
        const p = e?.payload;
        if (typeof p?.x === 'number' && typeof p?.y === 'number') {
          lastCollapsedPosRef.current = { x: p.x, y: p.y };
          try {
            localStorage.setItem(POS_KEY, JSON.stringify({ x: p.x, y: p.y }));
          } catch {
            // ignore
          }
        }
      });

      // Initial layout.
      const initPos = lastCollapsedPosRef.current;
      await win.setSize(new api.LogicalSize(COLLAPSED_SIZE, COLLAPSED_SIZE));
      if (initPos) await win.setPosition(new api.PhysicalPosition(initPos.x, initPos.y));

      // Make sure it’s visible.
      try {
        await win.show();
      } catch {
        // ignore
      }

      // Default: when launching in collapsed mode, make the window click-through
      // except for the bubble we toggle on hover. This prevents inactive regions
      // around the small icon from blocking the desktop.
      try {
        await win.setIgnoreCursorEvents(true as any);
        collapsedClickThroughRef.current = true;
      } catch {
        // Some platforms/versions may not support this; safe to ignore.
      }

      return () => {
        try {
          unlisten();
        } catch {
          // ignore
        }
      };
    })().catch((err) => console.error('[window]', err));

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const api = windowApiRef.current ?? (await tryLoadWindowApi());
      if (!api || cancelled) return;
      windowApiRef.current = api;

      const win = api.getCurrentWindow();
      await win.setResizable(false);
      await win.setAlwaysOnTop(true);

      scaleFactorRef.current = await win.scaleFactor();

      const pos = await win.outerPosition();
      const sf = scaleFactorRef.current || 1;

      if (expanded) {
        // Expand keeping the same visual center as the collapsed bubble.
        // Convert logical sizes to physical px to match outerPosition coords.
        const collapsedPx = Math.round(COLLAPSED_SIZE * sf);
        const expandedPx = Math.round(EXPANDED_SIZE * sf);

        // Current pos is collapsed top-left; compute its center.
        const centerX = pos.x + Math.round(collapsedPx / 2);
        const centerY = pos.y + Math.round(collapsedPx / 2);

        // Target expanded top-left so centers match.
        let x = centerX - Math.round(expandedPx / 2);
        let y = centerY - Math.round(expandedPx / 2);

        // Clamp to the current monitor work area (absolute coords), no extra margins.
        const mon = await api.currentMonitor();
        if (mon?.workArea) {
          const wa = mon.workArea;
          const minX = Math.round(wa.position.x);
          const minY = Math.round(wa.position.y);
          const maxX = Math.round(wa.position.x + wa.size.width - expandedPx);
          const maxY = Math.round(wa.position.y + wa.size.height - expandedPx);
          x = Math.max(minX, Math.min(x, maxX));
          y = Math.max(minY, Math.min(y, maxY));
        } else if (mon?.size && mon?.position) {
          const minX = Math.round(mon.position.x);
          const minY = Math.round(mon.position.y);
          const maxX = Math.round(mon.position.x + mon.size.width - expandedPx);
          const maxY = Math.round(mon.position.y + mon.size.height - expandedPx);
          x = Math.max(minX, Math.min(x, maxX));
          y = Math.max(minY, Math.min(y, maxY));
        }

        await win.setSize(new api.LogicalSize(EXPANDED_SIZE, EXPANDED_SIZE));
        await win.setPosition(new api.PhysicalPosition(x, y));

        // When expanded, fully interactive window.
        try {
          await win.setIgnoreCursorEvents(false as any);
          collapsedClickThroughRef.current = false;
        } catch {
          // ignore
        }
      } else {
        // Collapse keeping the same visual center as the expanded bubble.
        const collapsedPx = Math.round(COLLAPSED_SIZE * sf);
        const expandedPx = Math.round(EXPANDED_SIZE * sf);

        // Current pos is expanded top-left; compute its center.
        const centerX = pos.x + Math.round(expandedPx / 2);
        const centerY = pos.y + Math.round(expandedPx / 2);

        // Target collapsed top-left so centers match.
        let x = centerX - Math.round(collapsedPx / 2);
        let y = centerY - Math.round(collapsedPx / 2);

        // Clamp to current monitor work area (absolute coords), no extra margins.
        const mon = await api.currentMonitor();
        if (mon?.workArea) {
          const wa = mon.workArea;
          const minX = Math.round(wa.position.x);
          const minY = Math.round(wa.position.y);
          const maxX = Math.round(wa.position.x + wa.size.width - collapsedPx);
          const maxY = Math.round(wa.position.y + wa.size.height - collapsedPx);
          x = Math.max(minX, Math.min(x, maxX));
          y = Math.max(minY, Math.min(y, maxY));
        } else if (mon?.size && mon?.position) {
          const minX = Math.round(mon.position.x);
          const minY = Math.round(mon.position.y);
          const maxX = Math.round(mon.position.x + mon.size.width - collapsedPx);
          const maxY = Math.round(mon.position.y + mon.size.height - collapsedPx);
          x = Math.max(minX, Math.min(x, maxX));
          y = Math.max(minY, Math.min(y, maxY));
        }

        lastCollapsedPosRef.current = { x, y };
        try {
          localStorage.setItem(POS_KEY, JSON.stringify({ x, y }));
        } catch {
          // ignore
        }

        await win.setSize(new api.LogicalSize(COLLAPSED_SIZE, COLLAPSED_SIZE));
        await win.setPosition(new api.PhysicalPosition(x, y));

        // When collapsed, return to click-through (except bubble on hover).
        try {
          await win.setIgnoreCursorEvents(true as any);
          collapsedClickThroughRef.current = true;
        } catch {
          // ignore
        }
      }
    })().catch((err) => console.error('[window.layout]', err));

    return () => {
      cancelled = true;
    };
  }, [expanded]);

  const updatedAt = useMemo(() => {
    try {
      return format(new Date(selectedAgent.ts * 1000), 'HH:mm:ss');
    } catch {
      return String(selectedAgent.ts);
    }
  }, [selectedAgent.ts]);

  const lastActiveLabel = useMemo(() => {
    // Based purely on last event timestamp.
    if (!selectedAgent?.ts) return null;
    const txt = updatedAt;
    if (selectedAgent.state === 'inactive') {
      // If it was never used, keep it minimal.
      if (selectedAgent.detail === 'No events yet') return null;
      return `Last active: ${txt}`;
    }
    return null;
  }, [selectedAgent?.ts, selectedAgent?.state, selectedAgent?.detail, updatedAt]);

  const activityForSelected = activityByKey[selectedAgentKey] ?? [];

  function formatDurationSec(sec: number): string {
    if (!Number.isFinite(sec) || sec < 0) return '-';
    const s = Math.floor(sec);
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const r = s % 60;
    if (h > 0) return `${h}h ${m}m ${r}s`;
    if (m > 0) return `${m}m ${r}s`;
    return `${r}s`;
  }

  const sessionStartTs = useMemo(() => {
    const inst = selectedAgent.agentInstanceId;
    if (!inst) return null;
    // Find the first event of the latest contiguous block with this instanceId.
    let start: number | null = null;
    for (let i = activityForSelected.length - 1; i >= 0; i--) {
      const it = activityForSelected[i];
      if (it.agentInstanceId !== inst) {
        // We've crossed into an older session.
        break;
      }
      start = it.ts;
    }
    return start;
  }, [activityForSelected, selectedAgent.agentInstanceId]);

  // Intentionally removed: “Started at …” label (too much visual noise).
  const startedAtLabel = null;

  const completedInLabel = useMemo(() => {
    if (selectedAgent.state !== 'done') return null;
    if (!sessionStartTs) return null;
    const dur = selectedAgent.ts - sessionStartTs;
    return `Completed in ${formatDurationSec(dur)}`;
  }, [selectedAgent.state, selectedAgent.ts, sessionStartTs]);

  const activityRows = useMemo(() => {
    type Row =
      | { type: 'item'; itemIndex: number }
      | { type: 'divider'; label: string }
      | { type: 'summary'; label: string }
      | { type: 'hr' };

    const rows: Row[] = [];
    let curInstance: string | null = null;
    let curStart: number | null = null;

    for (let i = 0; i < activityForSelected.length; i++) {
      const it = activityForSelected[i];
      if (it.agentInstanceId !== curInstance) {
        if (curInstance != null) rows.push({ type: 'hr' });
        curInstance = it.agentInstanceId;
        curStart = it.ts;
        rows.push({ type: 'divider', label: `Session ${it.agentInstanceId}` });
      }

      rows.push({ type: 'item', itemIndex: i });

      if (it.state === 'done' && curStart != null) {
        const dur = it.ts - curStart;
        rows.push({ type: 'summary', label: `Completed in ${formatDurationSec(dur)}` });
        rows.push({ type: 'hr' });
        // Next event may belong to same instance (rare), but we still keep the divider.
      }
    }

    return rows;
  }, [activityForSelected]);

  const ariaLabel = expanded ? 'Collapse overlay' : 'Expand overlay';

  // Auto-select the most recently active instance.
  useEffect(() => {
    const best = order[0];
    if (best && best !== selectedAgentKey) setSelectedAgentKey(best);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [order]);

  const orbitN = Math.max(1, orbitVisible.length);


  // Manual dragging fallback for the collapsed bubble.
  // This avoids platform quirks where `startDragging()` only works in certain
  // contexts, and it also prevents click-to-expand from being flaky.
  const manualDragRef = useRef<{
    enabled: boolean;
    ready: boolean;
    didDrag: boolean;
    winStartX: number;
    winStartY: number;
    ptrStartX: number;
    ptrStartY: number;
    scaleFactor: number;
  } | null>(null);

  async function startManualDrag(e: React.PointerEvent) {
    if (expanded) return;
    if (e.button !== 0) return;
    const api = windowApiRef.current ?? (await tryLoadWindowApi());
    if (!api) return;
    windowApiRef.current = api;

    // While dragging, ensure the window is interactive (not click-through), otherwise
    // losing hover can flip ignoreCursorEvents mid-drag.
    try {
      await api.getCurrentWindow().setIgnoreCursorEvents(false as any);
      collapsedClickThroughRef.current = false;
    } catch {
      // ignore
    }

    // Seed immediately so pointer moves can start being processed without waiting
    // for async window queries.
    const seedPos = lastCollapsedPosRef.current;
    manualDragRef.current = {
      enabled: true,
      ready: true,
      didDrag: false,
      winStartX: seedPos?.x ?? 0,
      winStartY: seedPos?.y ?? 0,
      ptrStartX: e.screenX,
      ptrStartY: e.screenY,
      scaleFactor: scaleFactorRef.current || 1
    };

    // If we don't have a saved starting position yet, query it once.
    if (!seedPos) {
      const win = api.getCurrentWindow();
      const pos = await win.outerPosition();
      const sf = await win.scaleFactor();
      scaleFactorRef.current = sf;
      if (manualDragRef.current?.enabled) {
        manualDragRef.current.winStartX = pos.x;
        manualDragRef.current.winStartY = pos.y;
        manualDragRef.current.scaleFactor = sf;
      }
    }
  }

  async function moveManualDrag(e: React.PointerEvent) {
    if (expanded) return;
    const st = manualDragRef.current;
    if (!st?.enabled || !st.ready) return;

    const dx = e.screenX - st.ptrStartX;
    const dy = e.screenY - st.ptrStartY;
    const dist = Math.hypot(dx, dy);
    if (dist < DRAG_THRESHOLD_PX) return;

    pointerRef.current.didDrag = true;
    st.didDrag = true;

    // Cancel the 80ms expand fallback once we have a real drag.
    if (expandFallbackTimerRef.current) {
      window.clearTimeout(expandFallbackTimerRef.current);
      expandFallbackTimerRef.current = null;
    }

    const api = windowApiRef.current ?? (await tryLoadWindowApi());
    if (!api) return;
    windowApiRef.current = api;

    const sf = st.scaleFactor || scaleFactorRef.current || 1;
    const x = Math.round(st.winStartX + dx * sf);
    const y = Math.round(st.winStartY + dy * sf);

    // Deterministic coalescing: keep only latest position, and run a single async loop
    // that applies updates in-order (never concurrent setPosition calls).
    dragPendingPosRef.current = { x, y };

    const scheduleLoop = () => {
      if (dragLoopScheduledRef.current) return;
      dragLoopScheduledRef.current = true;
      window.requestAnimationFrame(() => {
        dragLoopScheduledRef.current = false;
        void flushPendingDragPos(api);
      });
    };

    scheduleLoop();
  }

  async function flushPendingDragPos(api: WindowApi) {
    if (dragSetPosInFlightRef.current) return;
    const pending = dragPendingPosRef.current;
    if (!pending) return;

    dragSetPosInFlightRef.current = true;
    try {
      // Apply the latest pending position
      await api.getCurrentWindow().setPosition(new api.PhysicalPosition(pending.x, pending.y));
      lastCollapsedPosRef.current = { x: pending.x, y: pending.y };
      try {
        localStorage.setItem(POS_KEY, JSON.stringify({ x: pending.x, y: pending.y }));
      } catch {
        // ignore
      }
    } catch (err) {
      console.error('[window.manualDrag]', err);
    } finally {
      dragSetPosInFlightRef.current = false;
    }

    // If pointer moves happened while we were awaiting, there may be a newer target.
    const next = dragPendingPosRef.current;
    if (next && (next.x !== pending.x || next.y !== pending.y)) {
      // Schedule another frame to apply the newest.
      if (!dragLoopScheduledRef.current) {
        dragLoopScheduledRef.current = true;
        window.requestAnimationFrame(() => {
          dragLoopScheduledRef.current = false;
          void flushPendingDragPos(api);
        });
      }
    }
  }

  function endManualDrag() {
    if (manualDragRef.current) manualDragRef.current.enabled = false;
    // Stop any scheduled position updates.
    dragLoopScheduledRef.current = false;
    dragPendingPosRef.current = null;
    dragSetPosInFlightRef.current = false;
  }

  function onPointerCancel() {
    if (expanded) return;
    if (expandFallbackTimerRef.current) {
      window.clearTimeout(expandFallbackTimerRef.current);
      expandFallbackTimerRef.current = null;
    }
    endManualDrag();
    pointerRef.current.isDown = false;
    pointerRef.current.didDrag = false;
  }

  // After a drag ends in collapsed mode, clamp the window to the visible work area
  // and persist the exact final position (no snap to edges).
  async function finalizeCollapsedDrop() {
    try {
      const api = windowApiRef.current ?? (await tryLoadWindowApi());
      if (!api) return;
      windowApiRef.current = api;

      const win = api.getCurrentWindow();
      // Current window position is in physical pixels
      const pos = await win.outerPosition();
      // Prefer monitor work area (excludes menu bar / dock) when available
      const mon = await api.currentMonitor();
      const sf = (scaleFactorRef.current || (await win.scaleFactor()) || 1) as number;
      const margin = 0; // allow exact placement without extra padding
      const collapsedPx = Math.round(COLLAPSED_SIZE * sf);

      let minX: number | null = null;
      let minY: number | null = null;
      let maxX: number | null = null;
      let maxY: number | null = null;

      if (mon?.workArea) {
        const wa = mon.workArea;
        minX = Math.round(wa.position.x + margin);
        minY = Math.round(wa.position.y + margin);
        maxX = Math.round(wa.position.x + wa.size.width - collapsedPx - margin);
        maxY = Math.round(wa.position.y + wa.size.height - collapsedPx - margin);
      } else if (mon?.size && mon?.position) {
        // Fallback to full monitor bounds if work area is unavailable
        minX = Math.round(mon.position.x + margin);
        minY = Math.round(mon.position.y + margin);
        maxX = Math.round(mon.position.x + mon.size.width - collapsedPx - margin);
        maxY = Math.round(mon.position.y + mon.size.height - collapsedPx - margin);
      }

      let x = pos.x;
      let y = pos.y;
      if (
        minX !== null &&
        minY !== null &&
        maxX !== null &&
        maxY !== null
      ) {
        x = Math.max(minX, Math.min(x, maxX));
        y = Math.max(minY, Math.min(y, maxY));
      }

      // Use PhysicalPosition to avoid subpixel rounding jitter on HiDPI
      await win.setPosition(new api.PhysicalPosition(x, y));
      lastCollapsedPosRef.current = { x, y };
      try {
        localStorage.setItem(POS_KEY, JSON.stringify({ x, y }));
      } catch {
        // ignore
      }
    } catch (err) {
      console.error('[window.finalizeCollapsedDrop]', err);
    }
  }

  function onPointerDown(e: React.PointerEvent) {
    if (expanded) return;
    const s = pointerRef.current;
    s.isDown = true;
    s.didDrag = false;
    s.startX = e.clientX;
    s.startY = e.clientY;

    // Ensure we keep receiving pointermove events even if the pointer slips
    // slightly outside the circle while dragging.
    try {
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    } catch {
      // ignore
    }

    // Prepare manual dragging.
    void startManualDrag(e);
  }

  function onBubblePointerDown(e: React.PointerEvent) {
    // Prevent outside-click handler from firing when interacting with the bubble.
    e.stopPropagation();
    onPointerDown(e);

    // Focus window immediately and schedule a first-click expand fallback
    // (helps when the first click only focuses the window on macOS).
    void (async () => {
      try {
        const api = windowApiRef.current ?? (await tryLoadWindowApi());
        if (api) await api.getCurrentWindow().setFocus();
      } catch {
        // ignore
      }
    })();

    // Fallback: if not dragged shortly after press, expand.
    if (!expanded) {
      if (expandFallbackTimerRef.current) window.clearTimeout(expandFallbackTimerRef.current);
      expandFallbackTimerRef.current = window.setTimeout(() => {
        if (!expandedRef.current && !pointerRef.current.didDrag) setExpanded(true);
      }, 80);
    }
  }

  function onPointerUp() {
    if (expanded) return;
    const s = pointerRef.current;
    s.isDown = false;

    if (expandFallbackTimerRef.current) {
      window.clearTimeout(expandFallbackTimerRef.current);
      expandFallbackTimerRef.current = null;
    }

    endManualDrag();

    if (s.didDrag) {
      // Drag gesture: do not toggle.
      // Clamp to the visible work area and persist the exact drop position.
      void finalizeCollapsedDrop();
      s.didDrag = false;

      // Restore click-through after drag ends.
      void (async () => {
        try {
          const api = windowApiRef.current ?? (await tryLoadWindowApi());
          if (!api) return;
          await api.getCurrentWindow().setIgnoreCursorEvents(true as any);
          collapsedClickThroughRef.current = true;
        } catch {
          // ignore
        }
      })();
      return;
    }

    // Single-click should always expand.
    setExpanded(true);
    s.didDrag = false;
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      setExpanded((v) => !v);
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      // Priority order:
      // 1) close activity
      // 2) close settings
      // 3) close orbit/expanded bubble
      if (activityOpen) {
        setActivityOpen(false);
        return;
      }
      if (settingsOpen) {
        setSettingsOpen(false);
        return;
      }
      if (expanded) {
        setExpanded(false);
      }
    }
  }

  return (
    <div
      className={expanded ? 'overlayRoot expanded' : 'overlayRoot collapsed'}
      onPointerDown={(e) => {
        // Only close when clicking the empty background.
        if (!expanded) return;
        if (e.target === e.currentTarget) setExpanded(false);
      }}
    >
      <div
        className={expanded ? 'bubble expanded' : 'bubble'}
        role="button"
        tabIndex={0}
        onPointerDown={onBubblePointerDown}
        onPointerMove={(e) => {
          if (expanded) return;
          void moveManualDrag(e);
        }}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerCancel}
        onMouseEnter={async () => {
          // While collapsed, disable click-through so the bubble is interactive.
          if (expanded) return;
          // If we're actively dragging, ignore hover toggles.
          if (manualDragRef.current?.enabled) return;
          try {
            const api = windowApiRef.current ?? (await tryLoadWindowApi());
            if (!api) return;
            await api.getCurrentWindow().setIgnoreCursorEvents(false as any);
            collapsedClickThroughRef.current = false;
          } catch {
            // ignore
          }
        }}
        onMouseLeave={async () => {
          // Restore click-through outside the bubble while collapsed.
          if (expanded) return;
          // If we're actively dragging, ignore hover toggles.
          if (manualDragRef.current?.enabled) return;
          try {
            const api = windowApiRef.current ?? (await tryLoadWindowApi());
            if (!api) return;
            await api.getCurrentWindow().setIgnoreCursorEvents(true as any);
            collapsedClickThroughRef.current = true;
          } catch {
            // ignore
          }
        }}
        onClick={(e) => {
          // Some platforms require a normal click handler (first click may focus the window).
          // Keep this as a fallback so expand is always single-click.
          if (expanded) return;
          if (pointerRef.current.didDrag) return;
          e.stopPropagation();
          setExpanded(true);
        }}
        onKeyDown={onKeyDown}
        aria-label={ariaLabel}
      >
        {!expanded ? (
          <>
            <div className="avatarWrap">
              <LottieCircle
                src={collapsedCandidates[0]}
                srcList={collapsedCandidates}
                size={84}
                loop={collapsedLoop}
                playing={collapsedPlaying}
              />
            </div>
            {selectedAgent.agentKey === 'system:idle' ? (
              <div className="noSessions">No active sessions</div>
            ) : null}
          </>
        ) : (
          <div className="orbitArea">
            {!updateDismissed && updateStatus.state === 'available' ? (
              <div
                className="updateBanner"
                onClick={(e) => e.stopPropagation()}
                onPointerDown={(e) => e.stopPropagation()}
              >
                <div className="updateBannerLeft">
                  <div className="updateBannerTitle">Update available</div>
                  <div className="updateBannerSub">
                    {updateStatus.version ? `New: ${updateStatus.version}` : 'A new version is ready.'}
                    {appVersion ? ` · Current: ${appVersion}` : ''}
                  </div>
                </div>
                <div className="updateBannerRight">
                  <button type="button" onClick={() => void downloadAndInstallUpdate()}>
                    Update
                  </button>
                  <button
                    type="button"
                    className="updateBannerClose"
                    aria-label="Dismiss update banner"
                    onClick={() => {
                      setUpdateDismissed(true);
                      try {
                        localStorage.setItem(UPDATE_DISMISS_KEY, '1');
                      } catch {
                        // ignore
                      }
                    }}
                  >
                    ×
                  </button>
                </div>
              </div>
            ) : null}

            {!updateDismissed && updateStatus.state === 'downloading' ? (
              <div
                className="updateBanner"
                onClick={(e) => e.stopPropagation()}
                onPointerDown={(e) => e.stopPropagation()}
              >
                <div className="updateBannerLeft">
                  <div className="updateBannerTitle">Downloading update…</div>
                  <div className="updateBannerSub">
                    {typeof updateStatus.progress === 'number'
                      ? `${Math.round(updateStatus.progress)}%`
                      : 'Please wait.'}
                  </div>
                </div>
                <div className="updateBannerRight">
                  <button
                    type="button"
                    className="updateBannerClose"
                    aria-label="Dismiss update banner"
                    onClick={() => {
                      setUpdateDismissed(true);
                      try {
                        localStorage.setItem(UPDATE_DISMISS_KEY, '1');
                      } catch {
                        // ignore
                      }
                    }}
                  >
                    ×
                  </button>
                </div>
              </div>
            ) : null}

            {!updateDismissed && updateStatus.state === 'error' ? (
              <div
                className="updateBanner updateBannerError"
                onClick={(e) => e.stopPropagation()}
                onPointerDown={(e) => e.stopPropagation()}
              >
                <div className="updateBannerLeft">
                  <div className="updateBannerTitle">Update failed</div>
                  <div className="updateBannerSub" title={updateStatus.message}>
                    {updateStatus.message}
                  </div>
                </div>
                <div className="updateBannerRight">
                  <button
                    type="button"
                    onClick={() => {
                      // Allow retry.
                      updateCheckedRef.current = false;
                      void checkForUpdatesOnce();
                    }}
                  >
                    Retry
                  </button>
                  <button
                    type="button"
                    className="updateBannerClose"
                    aria-label="Dismiss update banner"
                    onClick={() => {
                      setUpdateDismissed(true);
                      try {
                        localStorage.setItem(UPDATE_DISMISS_KEY, '1');
                      } catch {
                        // ignore
                      }
                    }}
                  >
                    ×
                  </button>
                </div>
              </div>
            ) : null}

            <div className="orbitRing" aria-hidden />

            {toast ? <div className="toast">{toast}</div> : null}

            <div className="orbitPlanets" aria-hidden={false}>
              <div ref={orbitSpinRef} className="orbitPlanetsSpin">
                {orbitVisible.map((k, idx) => {
                  const ev = byKey[k];
                  const candidates = lottieCandidatesForAgentState(ev.agentFamily, ev.state, 'planet');
                  const loop = ev.state !== 'done';
                  const selected = k === selectedAgentKey;
                  const displayName = customAgentNames[k] || ev.agentName;
                  // Performance: only animate the selected planet.
                  const playing = selected;
                  return (
                    <div
                      key={k}
                      className={selected ? 'planetWrap selected' : 'planetWrap'}
                      style={{
                        // Pass CSS vars for orbit placement.
                        ['--i' as any]: idx,
                        ['--n' as any]: orbitN
                      }}
                      onClick={(e) => {
                        e.stopPropagation();
                        setSelectedAgentKey(k);
                      }}
                    >
                      <div className="planetUpright">
                        {ev.state === 'inactive' ? (
                          <button
                            type="button"
                            className="planetDismiss"
                            aria-label="Remove inactive avatar"
                            onPointerDown={(e) => e.stopPropagation()}
                            onClick={(e) => {
                              e.stopPropagation();
                              dismissInactiveAvatar(k);
                            }}
                          >
                            ×
                          </button>
                        ) : null}
                        <button
                          className="planetBtn"
                          aria-label={`Select ${ev.agentName}`}
                          type="button"
                          onPointerDown={(e) => e.stopPropagation()}
                          onClick={(e) => {
                            e.stopPropagation();
                            setSelectedAgentKey(k);
                          }}
                        >
                          <LottieCircle src={candidates[0]} srcList={candidates} size={72} loop={loop} playing={playing} />
                        </button>

                        <div className="planetMeta" aria-hidden={false}>
                          <input
                            className="planetName"
                            value={displayName}
                            onChange={(e) => setAgentDisplayName(k, e.target.value)}
                            onClick={(e) => e.stopPropagation()}
                            onPointerDown={(e) => e.stopPropagation()}
                            spellCheck={false}
                            aria-label="IDE name"
                          />
                          <div className="planetStatus">{ev.state}</div>
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>

            {/* Center content: selected agent + settings */}
            <div
              className="sun"
              role="button"
              tabIndex={0}
              aria-label="Selected agent details (click to close)"
              onClick={(e) => {
                e.stopPropagation();
                setExpanded(false);
              }}
            >
              <div className="sunTitle">{selectedAgentName}</div>
              <div className="sunSub">{selectedAgent.state}</div>
              {selectedProjectName ? <div className="sunUpdated">Project {selectedProjectName}</div> : null}
              {selectedAgent.state === 'inactive' ? (
                lastActiveLabel ? (
                  <div className="sunUpdated">{lastActiveLabel}</div>
                ) : null
              ) : startedAtLabel ? (
                <div className="sunUpdated">{startedAtLabel}</div>
              ) : null}
              {completedInLabel ? <div className="sunUpdated">{completedInLabel}</div> : null}
              {/* Intentionally keep the center minimal; details live in Activity. */}

              {topApproval ? (
                <div
                  className="approvalCard"
                  onClick={(e) => e.stopPropagation()}
                  onPointerDown={(e) => e.stopPropagation()}
                >
                  <div className="approvalTitle">Awaiting approval</div>
                  <div className="approvalSummary">{topApproval.summary}</div>
                  <div className="approvalMeta">
                    {topApproval.agentFamily} · {topApproval.hook}
                  </div>

                  <div className="approvalActions">
                    {(topApproval.decisionOptions?.length
                      ? topApproval.decisionOptions
                      : ['allow', 'deny', 'ask']
                    ).map((opt) => (
                      <button
                        key={opt}
                        type="button"
                        onClick={() => void postApprovalDecision(topApproval.id, opt)}
                      >
                        {opt}
                      </button>
                    ))}
                  </div>

                  {/* Secondary row (smaller): auto-approve toggle for this IDE family */}
                  {(() => {
                    const fam = topApproval.agentFamily;
                    const enabled = Boolean(autoApproveFamilies?.[fam]);

                    if (!enabled) {
                      return (
                        <div className="approvalSecondaryActions">
                          <button
                            className="approvalSecondary"
                            type="button"
                            onClick={() => {
                              // Approve this request now.
                              postApprovalDecision(topApproval.id, 'allow');
                              // Persist auto-approve for future requests for this family.
                              setAutoApproveForFamily(fam, true);
                            }}
                          >
                            Auto-approve future requests for this IDE
                          </button>
                        </div>
                      );
                    }

                    return (
                      <div className="approvalSecondaryActions">
                        <button
                          className="approvalSecondary"
                          type="button"
                          onClick={() => setAutoApproveForFamily(fam, false)}
                        >
                          Auto-approve is ON · Disable
                        </button>
                      </div>
                    );
                  })()}
                </div>
              ) : null}

              <button
                className="settingsBtn"
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  const opening = !activityOpen;
                  setActivityOpen(opening);
                  if (opening) {
                    setSettingsOpen(false);
                    setApprovalsOpen(false);
                  }
                }}
                aria-label="Activity"
              >
                <span aria-hidden>Activity</span>
              </button>

              <button
                className="settingsBtn"
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  const opening = !approvalsOpen;
                  setApprovalsOpen(opening);
                  if (opening) {
                    setSettingsOpen(false);
                    setActivityOpen(false);
                  }
                }}
                aria-label="Approvals"
              >
                <span aria-hidden>Approvals</span>
              </button>

              <button
                className="settingsBtn icon"
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  const opening = !settingsOpen;
                  setSettingsOpen(opening);
                  if (opening) {
                    setActivityOpen(false);
                    setApprovalsOpen(false);
                  }
                  if (opening) void refreshIntegrations();
                }}
                aria-label="Settings"
              >
                <span aria-hidden>⛭</span>
              </button>

              {approvalsOpen ? (
                <div
                  className="approvalsPanel"
                  role="dialog"
                  aria-label="Approvals"
                  onClick={(e) => e.stopPropagation()}
                  onPointerDown={(e) => e.stopPropagation()}
                >
                  <div className="approvalsHeader">
                    <div>
                      <div className="settingsTitle">Approvals</div>
                      <div className="settingsSub">Auto-approve tool approvals per IDE.</div>
                    </div>

                    <button
                      className="approvalsClose"
                      type="button"
                      aria-label="Close approvals"
                      onClick={() => setApprovalsOpen(false)}
                    >
                      <span aria-hidden>×</span>
                    </button>
                  </div>

                  <div className="autoApproveCard">
                    <div className="approvalTitle">Auto-approve (per IDE)</div>

                    {enabledFamiliesForApprovals.length ? (
                      <div className="autoApproveGrid">
                        {enabledFamiliesForApprovals.map((fam) => {
                          const enabled = Boolean(autoApproveFamilies?.[fam]);
                          return (
                            <button
                              key={fam}
                              type="button"
                              className={enabled ? 'autoApproveBtn on' : 'autoApproveBtn'}
                              onClick={() => setAutoApproveForFamily(fam, !enabled)}
                              aria-label={`Toggle auto-approve for ${fam}`}
                            >
                              <span className="autoApproveFam">{fam}</span>
                              <span className="autoApproveState">{enabled ? 'ON' : 'OFF'}</span>
                            </button>
                          );
                        })}
                      </div>
                    ) : (
                      <div className="settingsSub">No enabled IDE integrations.</div>
                    )}
                  </div>
                </div>
              ) : null}

              {settingsOpen ? (
                <div
                  className="settingsPanel"
                  role="dialog"
                  aria-label="Integrations settings"
                  onClick={(e) => e.stopPropagation()}
                  onPointerDown={(e) => e.stopPropagation()}
                >
                  <div className="settingsHeader">
                    <div>
                      <div className="settingsTitle">Integrations</div>
                      <div className="settingsSub">Connect your IDE agent.</div>
                    </div>

                    <button
                      className="settingsIconBtn"
                      type="button"
                      aria-label="Close settings"
                      onClick={() => {
                        setSettingsOpen(false);
                      }}
                    >
                      <span aria-hidden>×</span>
                    </button>
                  </div>

                  {integrationsErr ? <div className="settingsErr">{integrationsErr}</div> : null}

                  {integrations ? (
                    <div className="settingsList">
                      {INTEGRATION_ITEMS.map((it) => {
                        const row = integrations[it.key];
                        const canEnable = row.supported && row.detected && !row.enabled;
                        const diag = (() => {
                          // Keep UI compact: only a tooltip, no new visible text.
                          const parts: string[] = [];
                          if (integrations.runner && integrations.runner.exists === false) parts.push('runner missing');
                          if (row.configEnabled === false) parts.push('hooks not installed');
                          if (row.configEnabled === true) parts.push('hooks installed');
                          if (row.shim?.exists === false) parts.push('shim missing');
                          if (row.shim?.path) parts.push(`shim: ${row.shim.path}`);
                          return parts.length ? parts.join(' · ') : undefined;
                        })();
                        return (
                          <div key={it.key} className="settingsRow">
                            <div className="settingsLeft">
                              <div className="settingsName">
                                {it.key === 'vscode' ? (
                                  <>
                                    <button
                                      type="button"
                                      className="twistyBtn"
                                      aria-label={vscodePanelOpen ? 'Hide VS Code workspaces' : 'Show VS Code workspaces'}
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        setVscodePanelOpen((v) => !v);
                                      }}
                                    >
                                    </button>
                                    <span>VS Code</span><br></br>
                                    <span className="settingsSuffix">(.github/hooks)</span>
                                  </>
                                ) : it.key === 'cline' ? (
                                  <>
                                    <button
                                      type="button"
                                      className="twistyBtn"
                                      aria-label={clinePanelOpen ? 'Hide Cline workspaces' : 'Show Cline workspaces'}
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        setClinePanelOpen((v) => !v);
                                      }}
                                    >
                                    </button>
                                    <span>Cline</span><br></br>
                                    <span className="settingsSuffix">(.clinerules/hooks)</span>
                                  </>
                                ) : (
                                  it.label
                                )}
                              </div>
                              <div className="settingsMeta">
                                <span title={diag}>
                                  {row.enabled ? 'Enabled' : row.detected ? 'Detected' : 'Not found'}
                                </span>
                              </div>
                              {it.key === 'vscode' && vscodePanelOpen ? (
                                <div className="vscodeWs">
                                  {Array.isArray((row as any).workspaces) && (row as any).workspaces.length ? (
                                    <div className="wsList">
                                      {(row as any).workspaces.map((s: any, idx: number) => {
                                        const fullPath = String(s?.workspacePath ?? '');
                                        const parts = fullPath.split(/[\\/]/).filter(Boolean);
                                        const displayName = parts[parts.length - 1] || fullPath;
                                        return (
                                          <div key={`${fullPath}-${idx}`} className="wsItem">
                                            <span className="wsPath" title={fullPath}>{displayName}</span>
                                            <div className="wsActions">
                                              <button
                                                type="button"
                                                onClick={() => void disableVscodeWorkspacePath(fullPath)}
                                              >
                                                Disable
                                              </button>
                                            </div>
                                          </div>
                                        );
                                      })}
                                    </div>
                                  ) : (
                                    <div className="settingsSub">No workspaces!</div>
                                  )}
                                  <div className="addRepo">
                                    <div className="wsActions">
                                      <button type="button" onClick={() => void browseVscodeWorkspace()}>
                                        Add repo
                                      </button>
                                    </div>
                                  </div>
                                </div>
                              ) : null}
                              {it.key === 'cline' && clinePanelOpen ? (
                                <div className="vscodeWs">
                                  {Array.isArray((row as any).workspaces) && (row as any).workspaces.length ? (
                                    <div className="wsList">
                                      {(row as any).workspaces.map((s: any, idx: number) => {
                                        const fullPath = String(s?.workspacePath ?? '');
                                        const parts = fullPath.split(/[\\/]/).filter(Boolean);
                                        const displayName = parts[parts.length - 1] || fullPath;
                                        return (
                                          <div key={`${fullPath}-${idx}`} className="wsItem">
                                            <span className="wsPath" title={fullPath}>{displayName}</span>
                                            <div className="wsActions">
                                              <button
                                                type="button"
                                                onClick={() => void disableClineWorkspacePath(fullPath)}
                                              >
                                                Disable
                                              </button>
                                            </div>
                                          </div>
                                        );
                                      })}
                                    </div>
                                  ) : (
                                    <div className="settingsSub">No workspaces!</div>
                                  )}
                                  <div className="addRepo">
                                    <div className="wsActions">
                                      <button type="button" onClick={() => void browseClineWorkspace()}>
                                        Add repo
                                      </button>
                                    </div>
                                  </div>
                                </div>
                              ) : null}
                            </div>
                            <div className="settingsRight">
                                {it.key === 'vscode' ? (
                                  row.enabled ? (
                                    <button type="button" onClick={() => void disableAllVscodeWorkspaces()}>
                                      Disable all
                                    </button>
                                  ) : row.detected ? (
                                    <button
                                      type="button"
                                      disabled={!canEnable}
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        setVscodePanelOpen((v) => !v);
                                      }}
                                    >
                                      Enable
                                    </button>
                                  ) : (
                                    <span className="pill warn">Install first</span>
                                  )
                                ) : it.key === 'cline' ? (
                                  row.enabled ? (
                                    <button type="button" onClick={() => void disableAllClineWorkspaces()}>
                                      Disable all
                                    </button>
                                  ) : row.detected ? (
                                    <button
                                      type="button"
                                      disabled={!canEnable}
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        setClinePanelOpen((v) => !v);
                                      }}
                                    >
                                      Enable
                                    </button>
                                  ) : (
                                    <span className="pill warn">Install first</span>
                                  )
                                ) : row.enabled ? (
                                  <button type="button" onClick={() => void disableIntegration(it.key)}>
                                    Disable
                                  </button>
                                ) : row.detected ? (
                                  <button
                                    type="button"
                                    disabled={!canEnable}
                                    onClick={() => void enableIntegration(it.key)}
                                  >
                                    Enable
                                  </button>
                                ) : (
                                  <span className="pill warn">Install first</span>
                                )}
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  ) : (
                    <div className="settingsSub">(Run inside the Tauri app to configure.)</div>
                  )}

                  <div className="settingsActions">
                    <button type="button" onClick={() => void refreshIntegrations()}>
                      Refresh
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setSettingsOpen(false);
                      }}
                    >
                      Close
                    </button>
                  </div>
                </div>
              ) : null}

              {activityOpen ? (
                <div
                  className="activityPanel"
                  role="dialog"
                  aria-label="Agent activity"
                  onClick={(e) => e.stopPropagation()}
                  onPointerDown={(e) => e.stopPropagation()}
                >
                  <div className="settingsHeader">
                    <div>
                      <div className="settingsTitle">Activity</div>
                      <div className="settingsSub">Recent hook events for this IDE.</div>
                    </div>

                    <button
                      className="settingsIconBtn"
                      type="button"
                      aria-label="Close activity"
                      onClick={() => {
                        setActivityOpen(false);
                      }}
                    >
                      <span aria-hidden>×</span>
                    </button>
                  </div>

                  <div className="activityList">
                    {overflowKeys.length ? (
                      <div className="activityDivider">Overflow ({overflowKeys.length})</div>
                    ) : null}
                    {overflowKeys.slice(0, 16).map((k) => {
                      const ev = byKey[k];
                      if (!ev) return null;
                      return (
                        <button
                          key={`ov-${k}`}
                          type="button"
                          className="overflowRow"
                          onClick={() => {
                            setSelectedAgentKey(k);
                            setActivityOpen(false);
                          }}
                        >
                          <span className="overflowName">{ev.agentName}</span>
                          <span className="overflowMeta">
                            {ev.agentFamily} · {ev.state} · {ev.agentInstanceId}
                          </span>
                        </button>
                      );
                    })}

                    {activityRows.map((row, rIdx) => {
                      if (row.type === 'divider') {
                        return (
                          <div key={`div-${rIdx}`} className="activityDivider">
                            {row.label}
                          </div>
                        );
                      }
                      if (row.type === 'summary') {
                        return (
                          <div key={`sum-${rIdx}`} className="activitySummary">
                            {row.label}
                          </div>
                        );
                      }
                      if (row.type === 'hr') {
                        return <div key={`hr-${rIdx}`} className="activityHr" />;
                      }

                      const it = activityForSelected[row.itemIndex];
                      if (!it) return null;

                      const isLatest = row.itemIndex === activityForSelected.length - 1;
                      const next = activityForSelected[row.itemIndex + 1];
                      const start = it.ts;
                      const end = next ? next.ts : undefined;
                      const startTxt = format(new Date(start * 1000), 'HH:mm:ss');
                      const endTxt = end ? format(new Date(end * 1000), 'HH:mm:ss') : null;

                      const cls = `pipe ${it.state}`;

                      const verb = (() => {
                        const s = it.state;
                        const d = (it.detail ?? '').toLowerCase();
                        const isDenied = d.includes('denied');
                        const isAborted = d.includes('aborted');

                        const latestErrorLabel = () => {
                          if (isDenied) return 'Denied';
                          if (isAborted) return 'Aborted';
                          return 'Error';
                        };

                        const pastErrorVerb = () => {
                          if (isDenied) return 'Denied';
                          if (isAborted) return 'Aborted';
                          return 'Errored';
                        };

                        if (isLatest) {
                          if (s === 'thinking') return 'Thinking';
                          if (s === 'reading') return 'Reading';
                          if (s === 'editing') return 'Editing';
                          if (s === 'running') return 'Running';
                          if (s === 'awaiting') return 'Awaiting';
                          if (s === 'done') return 'Done';
                          if (s === 'error') return latestErrorLabel();
                          if (s === 'inactive') return 'Inactive';
                          return 'Working';
                        }

                        if (s === 'thinking') return 'Thought';
                        if (s === 'reading') return 'Read';
                        if (s === 'editing') return 'Edited';
                        if (s === 'running') return 'Ran';
                        if (s === 'awaiting') return 'Awaited';
                        if (s === 'done') return 'Done';
                        if (s === 'error') return pastErrorVerb();
                        if (s === 'inactive') return 'Inactive';
                        return 'Did';
                      })();

                      const hook = it.hook ? `${verb} (${it.hook})` : verb;

                      return (
                        <div key={`${it.ts}-${row.itemIndex}`} className="activityRow">
                          <div className="activityLeft">
                            <div className="activityHook">{hook}</div>
                            <div className="activityTime">
                              {startTxt}
                              {endTxt ? ` → ${endTxt}` : ''}
                            </div>
                          </div>

                          <div className="activityMid" aria-hidden>
                            <div className={cls} />
                          </div>

                          <div className="activityRight">
                            <div className="activityDetail">{it.detail ?? '-'}</div>
                            {it.projectName ? <div className="activityMeta">{it.projectName}</div> : null}
                          </div>
                        </div>
                      );
                    })}
                  </div>

                  <div className="settingsActions">
                    <button type="button" onClick={() => setActivityOpen(false)}>
                      Close
                    </button>
                  </div>
                </div>
              ) : null}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export default App;
