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

// ---------------- Agent Wrapped ----------------

export type WrappedRange = 'today' | 'past7';

export type WrappedCard1 = {
  agent_hours: number;
  projects_count: number;
  longest_run_s: number;
  thinking_pct: number;
  editing_pct: number;
  running_tools_pct: number;
};

export type WrappedProjectOption = {
  project_path: string;
  project_name: string;
  agent_hours: number;
};

export type WrappedCard2 = {
  project: WrappedProjectOption;
  prompted: number;
  prompt_chars: number;
  agent_hours: number;
  ide_split: Array<[string, number]>;
};

export type WrappedMetrics = {
  agent_hours: number;
  projects_count: number;
  files_count: number;
  sessions_count: number;
  prompts_count: number;
  avg_session_minutes: number;
  night_ratio: number;
  max_parallel_agents: number;
  error_ratio: number;
  approval_ratio: number;
  favourite_agent: string;
  favourite_model?: string;
};

export type WrappedCard3 = {
  archetype: {
    archetype_name: string;
    description: string;
  };
  metrics: WrappedMetrics;
};

export type WrappedOut = {
  range: WrappedRange;
  start_ts_s: number;
  end_ts_s: number;
  card1: WrappedCard1;
  card2: WrappedCard2;
  card3: WrappedCard3;
  projects: WrappedProjectOption[];
};
