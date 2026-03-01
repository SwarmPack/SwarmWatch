export type AgentState =
  | 'inactive'
  | 'idle'
  | 'thinking'
  | 'reading'
  | 'editing'
  | 'running'
  | 'awaiting'
  | 'error'
  | 'done';

// NOTE: keep this open-ended so we can support new IDE agents without
// touching core types. Known families today: cursor|claude|windsurf.
export type AgentFamily = string;

// Unique instance key, e.g. "cursor:668320d2-..." or "claude:abc123".
export type AgentKey = string;

export type AgentName = string;

export type AgentStateEvent = {
  type: 'agent_state';
  agentFamily: AgentFamily;
  agentInstanceId: string;
  agentKey: AgentKey;
  agentName: AgentName;
  state: AgentState;
  detail?: string;
  // Optional metadata provided by runners/adapters.
  hook?: string; // e.g. beforeReadFile | PreToolUse | pre_run_command
  projectName?: string; // basename only (no full path)
  ts: number; // epoch seconds
};

export type ActivityItem = {
  ts: number; // epoch seconds
  agentInstanceId: string;
  hook?: string;
  state: AgentState;
  detail?: string;
  projectName?: string;
};

export type ApprovalRequest = {
  id: string;
  createdAt: number;
  status: 'pending' | 'approved' | 'denied' | 'expired';
  decision?: 'allow' | 'deny' | 'ask';
  decidedAt?: number;
  reason?: string;
  agentKey: AgentKey;
  agentFamily: AgentFamily;
  agentInstanceId: string;
  hook: string;
  summary: string;
  raw: unknown;

  // UI rendering: which decisions to show as buttons.
  // IMPORTANT: this avoids hardcoding allow/deny/ask in the UI.
  decisionOptions?: string[];
  // Policy metadata: which decisions are considered a denial.
  denyOptions?: string[];
};

export type SettingsMessage = {
  type: 'settings';
  autoApproveFamilies: Record<string, boolean>;
};

export type WsMessage =
  | AgentStateEvent
  | { type: 'approvals'; pending: ApprovalRequest[] }
  | SettingsMessage;
