# Agent Wrapped

SwarmWatch computes a lightweight “Wrapped” summary from the local SQLite event database and exposes it via the control plane.

## Endpoint

`GET http://127.0.0.1:4100/wrapped?range=today|past7&project_path=<optional>`

Response:

```jsonc
{ "ok": true, "data": {
  "range": "today",
  "start_ts_s": 1710000000,
  "end_ts_s": 1710003600,
  "card1": {
    "agent_hours": 1.4,
    "projects_count": 2,
    "longest_run_s": 312,
    "thinking_pct": 24,
    "editing_pct": 51,
    "running_tools_pct": 25
  },
  "card2": {
    "project": { "project_path": "/abs/path", "project_name": "repo", "agent_hours": 1.1 },
    "prompted": 12,
    "prompt_chars": 5200,
    "agent_hours": 1.1,
    "ide_split": [["cursor", 70], ["vscode", 30]]
  },
  "card3": {
    "archetype": { "archetype_name": "The Builder", "description": "…" },
    "metrics": {
      "agent_hours": 1.4,
      "projects_count": 2,
      "files_count": 22,
      "sessions_count": 3,
      "prompts_count": 12,
      "avg_session_minutes": 18.4,
      "night_ratio": 0.1,
      "max_parallel_agents": 2,
      "error_ratio": 0.04,
      "approval_ratio": 0.92,
      "favourite_agent": "cursor",
      "favourite_model": "claude-3-5-sonnet"
    }
  },
  "projects": [
    { "project_path": "/abs/path", "project_name": "repo", "agent_hours": 1.1 }
  ]
}}
```

Notes:
* `range=today` uses the user’s local day boundary.
* `range=past7` is a rolling 7×24h window ending “now”.
* `project_path` (optional) forces card2 to be computed for a specific project.

## Data sources

Metrics are derived from the local DB (see `src-tauri/src/db.rs` and `src-tauri/src/wrapped.rs`).

At a high level:
* Agent hours: wall-clock duration attributed to non-idle states
* Project count: distinct `project_path`
* Files count: distinct file paths observed in hook metadata
* Prompts count / chars: aggregated from captured prompt payloads
* Approvals ratio: approvals approved / total decisions (approved+denied)

## Archetype selection (deterministic)

The archetype is chosen from a scoring function over:
* time distribution across thinking/editing/running
* error ratio
* approvals ratio
* breadth: files/projects/sessions
* max parallel agents

If multiple archetypes tie, a deterministic hash-based tiebreaker is used so the same input produces the same archetype.

## UI

The overlay UI (React) opens **Wrapped** from the center “sun” panel and displays three mini-cards.

Share:
* Renders a fixed **1080×1920** hidden DOM node to PNG using `html-to-image`
* Saves to **Downloads** by default (falls back to a Save dialog)
* Copies the PNG to clipboard when available
