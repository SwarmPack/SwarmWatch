# Telemetry Handoff (PostHog) — rationale + implementation guide

This document is written to hand to another agent.

## Goal (what we’re shipping)

Add minimal PostHog analytics to SwarmWatch (Tauri + React UI + Rust control plane):

1) **Daily unique avatar sessions** per distinct user
   - Count **unique IDE-avatar sessions spun up per day** per anonymous user.
   - Avoid double counting across: inactivity/reactivation, orbit eviction, UI close/reopen, app restart.
   - Supported agent families only: **`cursor` `claude` `cline` `vscode`**.

2) **UI click tracking**
   - Count clicks (or open/close toggles) for 4 UI buttons:
     - Recap (Wrapped)
     - Approvals
     - Audit trail (Activity)
     - Settings

## Hard constraints

Telemetry must be **minimal** and must not include sensitive data:

- DO NOT send prompts, tool payloads, code, file paths, commands, raw approval payloads, workspace roots, or project paths.
- Only send small fixed enums/labels + anonymous identifiers.
- Must be **non-blocking / fire-and-forget**:
  - enqueue locally
  - flush asynchronously
  - ignore errors / timeouts
  - rate-limit / batch

## What’s already implemented (and why)

### 1) `src-tauri/src/settings.rs`

Already added:

- `telemetry_distinct_id: Option<String>` on `SwarmWatchSettings`.
  - **Why Option?** Allows older settings.json files to deserialize without breaking.
  - **Why `#[serde(default)]`?** Missing field becomes `None` rather than error.
  - **Why `skip_serializing_if = "Option::is_none"`?** Keeps settings.json clean until telemetry is used.

- `pub fn telemetry_distinct_id() -> Result<String, String>`
  - Reads `settings.json` via `read_settings()`.
  - If `telemetry_distinct_id` exists and is non-empty → returns it.
  - Else generates `uuid::Uuid::new_v4().to_string()` → stores back via `write_settings()`.
  - **Why persisted?** To keep a stable PostHog `distinct_id` across restarts so “per-day unique” counts don’t reset.
  - **Why UUID v4?** Random, non-user-identifying, no entropy from machine info.

Security note: this is a **local anonymous device id**, not tied to accounts.

### 2) `src-tauri/src/lib.rs`

Already added:

- Tauri command `telemetry_context()` returning:

  ```ts
  { distinctId: string; platform: 'macos'|'windows'|'linux'|'other' }
  ```

  - `distinct_id` comes from `settings::telemetry_distinct_id()`.
  - `platform` computed via `cfg!(target_os = ...)`.
  - Command is registered in `.invoke_handler(...)`.

Why this exists:

- Frontend should not invent ids.
- Frontend can stay browser-preview compatible (only call invoke when in Tauri).
- Minimizes telemetry fields: stable id + platform.

## Key design decisions (the “why”, including edge cases)

### A) Why persistence is required for daily unique avatar sessions

Requirement A is **“unique sessions per distinct user per day without double counting across restart/reactivation/etc.”**

If we only emitted events in-memory:

- App restart would forget what was already counted → **double count**.
- UI unmount/remount (or WebView reload) would forget → **double count**.
- Inactivity → active transitions could re-trigger “new session” heuristics → **double count**.

So we need a persistent **set membership** structure keyed by:

```
(day, agent_family, agent_key)
```

…stored locally, and used only to decide whether to emit the daily unique event.

Important: `agent_key` is used **locally** for dedupe only and should **not be sent** to PostHog.

### B) Why compute daily session dedupe at control plane ingest (vs UI)

The control plane (`src-tauri/src/control_plane.rs`) is the canonical ingest point:

- It sees every normalized `POST /event` agent state update.
- It is independent of UI panels and orbit eviction.
- It is already where we persist high-volume event data (SQLite writer queue in `db.rs`).

UI-only dedupe is less reliable because:

- UI can be hidden/collapsed or even not connected if WebSocket reconnecting.
- UI state store truncation (`MAX_STORE`, `MAX_ACTIVITY`) can drop older keys.
- Orbit eviction is a UI decision (max 8 visible), not a real session boundary.

Therefore, the cleanest semantics are:

1) Control plane receives an `agent_state` event.
2) Control plane decides whether this `(day, agent_family, agent_key)` is new.
3) If new → emit exactly one PostHog event.

### C) Receipt time vs event timestamp

Time skew and buggy clients exist.

Evidence: frontend already treats “lastSeen” as **receipt time** (`useAgentStates.ts` comment) because clients can send wrong epoch units.

So for daily dedupe we should compute `day` using **local receipt time** on the SwarmWatch machine:

- don’t trust `ev.ts` (it could be epoch-ms or stale)
- use `SystemTime::now()` (Rust) or `Date.now()` (UI) for day boundary

This avoids:

- day misclassification from invalid `ts`
- double counts around midnight due to old events replayed

### D) Supported families only

Requirement restricts counting to these families:

```
cursor, claude, cline, vscode
```

So the dedupe logic must:

- `family = family.trim().to_lowercase()`
- if not in allowlist → ignore (do not emit)

Reason:

- avoids accidental data from `windsurf` or future integrations
- keeps analytics consistent and avoids schema drift

### E) Edge cases checklist (must pass)

Daily session counting must not double count when:

- **inactive → active**: same `agent_key` (family + instance id) shows new state, but it’s the same session.
- **orbit eviction**: session disappears from orbit/UI but is still “the same session”; do not recount.
- **app restart**: same `agent_key` resumes emitting events after restart; do not recount.
- **duplicates**: same event repeated / retries; set membership prevents recount.
- **unknown families**: ignore.
- **day boundary**: first event after midnight local time should count for new day (new set).

### F) Non-blocking requirements

Telemetry emission must never:

- block `/event` processing (which impacts live UI)
- block the UI thread
- throw hard errors

So:

- Use a queue + background flush (pattern already used by `DbWriter`).
- Use short network timeouts.
- Drop telemetry on repeated failures.

Implementation note (current frontend implementation):
- Telemetry is queued in memory and flushed asynchronously.
- The flush timer is **lazy**: starts on first queued event and stops when the queue is empty.

## Event schemas (what is sent to PostHog)

### Shared fields

Sent on all events:

- `distinct_id`: UUID v4 stored in `settings.json`
- `platform`: `macos|windows|linux|other`
- `app_version`: optional

PostHog capture envelope (reference):

```json
{
  "api_key": "<public_project_key>",
  "event": "<event_name>",
  "distinct_id": "<uuid>",
  "properties": {
    "platform": "macos",
    "app_version": "0.1.2",
    "...": "event-specific properties"
  }
}
```

### (A) Daily unique avatar session event

Event: `swarmwatch_avatar_session_daily`

Send only:

- `day`: `YYYY-MM-DD` (receipt-time local day label)
- `agent_family`: one of `cursor|claude|cline|vscode`

Do **NOT** send:

- `agent_key`
- instance ids
- project names/paths
- hook names

Important: this event is intentionally **not** “every session start” — it is “first time we see this unique avatar session key on this day”. This is what makes it stable under restarts / reactivation.

Local-only dedupe key:

```
(day, agent_family, agent_key)
```

### (B) UI click event

Event: `swarmwatch_ui_click`

Send:

- `target`: one of `recap|approvals|activity|settings`
- optionally `state`: `open|close` (if tracking toggles vs raw clicks)

Do **NOT** send:

- selected agent
- project name
- any UI text

## Recommended implementation plan (next agent)

There are two acceptable architectures; pick one and be consistent.

### Option 1 (recommended): Hybrid — control plane counts daily sessions, UI tracks clicks

Pros:

- Most correct semantics for “avatar sessions spun up”
- Not impacted by UI store eviction / orbit

Cons:

- Requires Rust → PostHog HTTP capture implementation

Steps:

1) **Add PostHog config**
   - Provide build-time vars for both frontend and Rust:
     - `VITE_POSTHOG_KEY`
     - `VITE_POSTHOG_HOST`
   - Both are “public” in PostHog terms (not secret) but keep in GH secrets/vars anyway.

   Host note: for US cloud the host is typically `https://us.i.posthog.com` (PostHog “US host”).

   IMPORTANT: `VITE_...` variables are exposed in the frontend bundle. That’s OK for a public PostHog project key.

   If you also send from Rust, either:
   - also use these same env vars in Rust at runtime (e.g. embed at build), or
   - store them in Tauri config / compile-time env.

2) **Persist daily session membership in SQLite**
   - Add migration v2 in `src-tauri/src/db.rs::migrate()`:

     ```sql
     CREATE TABLE IF NOT EXISTS telemetry_daily_sessions (
       day TEXT NOT NULL,
       agent_family TEXT NOT NULL,
       agent_key TEXT NOT NULL,
       first_seen_ts_s INTEGER NOT NULL,
       PRIMARY KEY(day, agent_family, agent_key)
     );
     ```

   - You can store plain `agent_key` locally; it never leaves the machine.

   Retention note: this table can be cleaned with the existing retention loop (e.g. delete rows older than 14 days) to keep DB bounded.

3) **Hook into `/event` ingest** (`control_plane.rs::post_event`)
   - After `ev.agent_key = normalize_agent_key(...)` is computed:
     - Check allowlist family.
     - Compute `day` from receipt-time `SystemTime::now()`.
     - Try `INSERT OR IGNORE` into `telemetry_daily_sessions`.
       - If insert happened (rows affected = 1) → this is the **first** time today.
     - Enqueue a telemetry capture job (non-blocking).

   Why this placement (line-by-line intent):
   - We only know `agent_key` after the server normalizes/sanitizes it.
   - Doing it here guarantees we count sessions even if UI is closed.
   - Doing it here avoids double counting from multiple UI re-connects.

4) **Add a telemetry sender queue in Rust**
   - Mirror `DbWriter` pattern:
     - `TelemetryWriter { queue: Vec<CapturePayload> }`
     - background task flush every N ms
     - batch up to M events
     - use `reqwest` or `ureq` with timeouts
   - Use PostHog capture endpoint:
     - `POST {host}/capture/`
     - payload:
       ```json
       {"api_key":"...","event":"...","distinct_id":"...","properties":{...}}
       ```
   - **Never await** inside `/event` handler.

   Failure policy:
   - If PostHog is unreachable, drop the batch.
   - Do not retry forever.
   - Keep queue size bounded (e.g. cap at 1k items; drop oldest).

   Concurrency/race note:
   - If multiple events arrive concurrently for a new `(day,family,agent_key)`, use SQLite's PRIMARY KEY + `INSERT OR IGNORE` to ensure only one emits.

5) **UI click tracking (PostHog JS)**
   - Add `posthog-js` dependency.
   - Init in `src/main.tsx` (or `src/telemetry.ts`) after calling `invoke('telemetry_context')`.
   - Must disable autocapture:
     - `autocapture: false`
     - `capture_pageview: false`
     - `disable_session_recording: true`
     - ensure no DOM text capture
   - Add `trackUiClick(target, state)` and call it from the 4 button onClicks in `App.tsx`.

   Important: configure PostHog JS to be as inert as possible:
   - disable automatic event capture
   - do not register super-properties beyond platform/version
   - do not use feature flags, surveys, session replay in this minimal phase

### Option 2: Frontend-only (simpler; semantics slightly weaker)

If you want *zero Rust network deps*, you can do:

- On every inbound WS `agent_state` message in `useAgentStates.ts`:
  - compute allowlist + day label
  - check local persisted membership set (localStorage or IndexedDB)
  - if new → `posthog.capture('swarmwatch_avatar_session_daily', {day, agent_family})`

This still meets “no double count across restart/reactivation” (because membership is persisted), but:

- it relies on UI being alive + connected

## GitHub Actions build-time env injection (release.yml)

`tauri.conf.json` runs `npm run build` via `beforeBuildCommand`.

So Vite only sees env vars present during `npx tauri build ...`.

Add to `.github/workflows/release.yml` in the **Build Tauri app** step:

```yml
env:
  VITE_POSTHOG_KEY: ${{ secrets.VITE_POSTHOG_KEY }}
  VITE_POSTHOG_HOST: ${{ secrets.VITE_POSTHOG_HOST }}
```

Also consider adding the same env vars to **`tauri dev`** in local development via `.env` (not committed), so telemetry can be exercised in dev builds.

Notes:

- PostHog “public” project keys are not truly secret, but GH secrets/vars keep configuration centralized.
- Do not print them in logs.

## Where to hook the 4 UI buttons (exact locations)

In `src/App.tsx` there are 4 buttons already:

- **Recap**: button with `aria-label="Recap"` (sets `wrappedOpen`)
- **Approvals**: button with `aria-label="Approvals"` (sets `approvalsOpen`)
- **Audit Trail**: button with `aria-label="Audit Trail"` (sets `activityOpen`)
- **Settings**: gear button with `aria-label="Settings"` (sets `settingsOpen`)

Add a call like:

```ts
trackUiClick('recap', opening ? 'open' : 'close')
```

…immediately after `opening` is computed (before any awaits).

## Minimal test plan

1) Run app in dev mode.
2) Trigger one real agent session (or use existing simulation scripts).
3) Verify:
   - exactly one `swarmwatch_avatar_session_daily` per `(day, family, session)` locally
   - multiple state changes do not increase count
4) Click the 4 buttons; verify 4 `swarmwatch_ui_click` captures with correct `target`.
5) Restart app; ensure daily session does **not** emit again for same session/day.

## Checklist of “do not accidentally ship”

- Do not enable PostHog autocapture.
- Do not send URL / page path.
- Do not attach agent detail strings.
- Do not attach approval summaries or `raw` payload.
- Do not attach file path arrays (`file_paths` exists in backend event schema—never forward it).

---

If you’re implementing this, prefer adding a small `telemetry` module (Rust and/or TS) and keep all capture calls centralized so future audits are easy.
