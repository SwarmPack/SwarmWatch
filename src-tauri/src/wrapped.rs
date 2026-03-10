//! Agent Wrapped stats computation.
//!
//! NOTE: All timestamps are epoch seconds.

use crate::db::{Db, RETENTION_DAYS};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

fn now_epoch_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy)]
pub enum RangeKind {
    Today,
    Past7,
}

impl RangeKind {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "today" => Some(Self::Today),
            "past7" | "7d" | "past_7_days" => Some(Self::Past7),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WrappedCard1 {
    pub agent_hours: f64,
    pub projects_count: i64,
    pub longest_run_s: i64,
    pub thinking_pct: i64,
    pub editing_pct: i64,
    pub running_tools_pct: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectOption {
    pub project_path: String,
    pub project_name: String,
    pub agent_hours: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WrappedCard2 {
    pub project: ProjectOption,
    pub prompted: i64,
    pub prompt_chars: i64,
    pub agent_hours: f64,
    pub ide_split: Vec<(String, i64)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WrappedMetrics {
    pub agent_hours: f64,
    pub projects_count: i64,
    pub files_count: i64,
    pub sessions_count: i64,
    pub prompts_count: i64,
    pub avg_session_minutes: f64,
    pub night_ratio: f64,
    pub max_parallel_agents: i64,
    pub error_ratio: f64,
    pub approval_ratio: f64,
    pub favourite_agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favourite_model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchetypeOut {
    pub archetype_name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WrappedCard3 {
    pub archetype: ArchetypeOut,
    pub metrics: WrappedMetrics,
}

#[derive(Debug, Clone, Serialize)]
pub struct WrappedOut {
    pub range: String,
    pub start_ts_s: i64,
    pub end_ts_s: i64,
    pub card1: WrappedCard1,
    pub card2: WrappedCard2,
    pub card3: WrappedCard3,
    pub projects: Vec<ProjectOption>,
}

fn local_midnight_epoch_s() -> i64 {
    // Simple local midnight computation using chrono is avoided to keep deps minimal.
    // We approximate by deriving midnight from the local offset of `time` crate? Not available.
    // So we compute midnight using system local time via std::time + libc.
    // This is macOS/Linux/Windows friendly enough.
    #[cfg(unix)]
    {
        use libc::{localtime_r, time_t, tm};
        use std::mem::MaybeUninit;

        unsafe {
            let now = now_epoch_s() as time_t;
            let mut out = MaybeUninit::<tm>::uninit();
            localtime_r(&now, out.as_mut_ptr());
            let mut t = out.assume_init();
            t.tm_hour = 0;
            t.tm_min = 0;
            t.tm_sec = 0;
            // mktime converts local tm to epoch.
            libc::mktime(&mut t) as i64
        }
    }

    #[cfg(not(unix))]
    {
        // Fallback: treat today as last 24h.
        now_epoch_s() - 24 * 3600
    }
}

pub fn compute_wrapped(db: &Db, range: RangeKind, project_path_override: Option<&str>) -> Result<WrappedOut, String> {
    let end = now_epoch_s();
    let start = match range {
        RangeKind::Today => local_midnight_epoch_s(),
        RangeKind::Past7 => end - RETENTION_DAYS * 24 * 3600,
    };

    let conn = db.lock().unwrap();

    // ---- Projects list (by agent time proxy: sum of durations between events) ----
    // V1 proxy: count events per project; later we can switch to duration-based.
    let mut stmt = conn
        .prepare(
            r#"
SELECT project_path, COALESCE(project_name, ''), COUNT(*) as c
FROM events
WHERE ts_s BETWEEN ?1 AND ?2
  AND project_path IS NOT NULL
  AND TRIM(project_path) <> ''
GROUP BY project_path, project_name
ORDER BY c DESC
LIMIT 25
"#,
        )
        .map_err(|e| format!("projects query prepare failed: {e}"))?;

    let mut projects: Vec<ProjectOption> = Vec::new();
    let rows = stmt
        .query_map(params![start, end], |row| {
            let path: String = row.get(0)?;
            let name: String = row.get(1)?;
            let count: i64 = row.get(2)?;
            Ok((path, name, count))
        })
        .map_err(|e| format!("projects query failed: {e}"))?;

    for r in rows {
        let (path, name, count) = r.map_err(|e| format!("projects row failed: {e}"))?;
        let display = if name.trim().is_empty() {
            // basename
            path.trim_end_matches(&['/', '\\'][..])
                .rsplit(&['/', '\\'][..])
                .next()
                .unwrap_or(&path)
                .to_string()
        } else {
            name
        };
        projects.push(ProjectOption {
            project_path: path,
            project_name: display,
            agent_hours: (count as f64) / 60.0, // very rough proxy
        });
    }

    if projects.is_empty() {
        // Never empty. Provide a placeholder.
        projects.push(ProjectOption {
            project_path: "".to_string(),
            project_name: "(no project)".to_string(),
            agent_hours: 0.0,
        });
    }

    let selected_project_path = project_path_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| projects[0].project_path.clone());

    // ---- Card1 base aggregates ----
    let projects_count: i64 = conn
        .query_row(
            r#"SELECT COUNT(DISTINCT project_path) FROM events WHERE ts_s BETWEEN ?1 AND ?2 AND project_path IS NOT NULL AND TRIM(project_path) <> ''"#,
            params![start, end],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let prompts_count: i64 = conn
        .query_row(
            r#"SELECT COUNT(*) FROM events WHERE ts_s BETWEEN ?1 AND ?2 AND prompt_chars IS NOT NULL"#,
            params![start, end],
            |r| r.get(0),
        )
        .unwrap_or(0);

    // Agent hours proxy: count events * 30s.
    let events_count: i64 = conn
        .query_row(
            r#"SELECT COUNT(*) FROM events WHERE ts_s BETWEEN ?1 AND ?2"#,
            params![start, end],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let agent_hours = (events_count as f64) * 30.0 / 3600.0;

    // Longest run proxy: max distance between prompt and next prompt/done/error/inactive per agent.
    // V1 simplified: 0.
    let longest_run_s = 0i64;

    // Percent buckets from tool_bucket counts.
    let thinking_c: i64 = conn
        .query_row(
            r#"SELECT COUNT(*) FROM events WHERE ts_s BETWEEN ?1 AND ?2 AND tool_bucket = 'thinking'"#,
            params![start, end],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let editing_c: i64 = conn
        .query_row(
            r#"SELECT COUNT(*) FROM events WHERE ts_s BETWEEN ?1 AND ?2 AND tool_bucket = 'editing'"#,
            params![start, end],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let running_c: i64 = conn
        .query_row(
            r#"SELECT COUNT(*) FROM events WHERE ts_s BETWEEN ?1 AND ?2 AND tool_bucket = 'running_tools'"#,
            params![start, end],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let total_b = thinking_c + editing_c + running_c;
    let pct = |v: i64| -> i64 {
        if total_b <= 0 {
            0
        } else {
            ((v as f64) * 100.0 / (total_b as f64)).round() as i64
        }
    };

    // Adjust to sum to 100.
    let thinking_pct = pct(thinking_c);
    let editing_pct = pct(editing_c);
    let mut running_tools_pct = 100 - thinking_pct - editing_pct;
    if running_tools_pct < 0 {
        running_tools_pct = 0;
    }
    if thinking_pct + editing_pct + running_tools_pct != 100 && total_b > 0 {
        // Force sum to 100.
        running_tools_pct = 100 - thinking_pct - editing_pct;
    }

    let card1 = WrappedCard1 {
        agent_hours: (agent_hours * 10.0).round() / 10.0,
        projects_count,
        longest_run_s,
        thinking_pct,
        editing_pct,
        running_tools_pct,
    };

    // ---- Card2 (project vibes) ----
    let prompted: i64 = conn
        .query_row(
            r#"SELECT COUNT(*) FROM events WHERE ts_s BETWEEN ?1 AND ?2 AND project_path=?3 AND prompt_chars IS NOT NULL"#,
            params![start, end, selected_project_path],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let prompt_chars: i64 = conn
        .query_row(
            r#"SELECT COALESCE(SUM(prompt_chars),0) FROM events WHERE ts_s BETWEEN ?1 AND ?2 AND project_path=?3"#,
            params![start, end, selected_project_path],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let agent_hours_project = (conn
        .query_row(
            r#"SELECT COUNT(*) FROM events WHERE ts_s BETWEEN ?1 AND ?2 AND project_path=?3"#,
            params![start, end, selected_project_path],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0) as f64)
        * 30.0
        / 3600.0;

    // IDE split (by event counts) limited to 2.
    let mut stmt = conn
        .prepare(
            r#"SELECT agent_family, COUNT(*) as c FROM events WHERE ts_s BETWEEN ?1 AND ?2 AND project_path=?3 GROUP BY agent_family ORDER BY c DESC"#,
        )
        .map_err(|e| format!("ide split prepare failed: {e}"))?;
    let mut ide_split: Vec<(String, i64)> = Vec::new();
    let mut total = 0i64;
    let rows = stmt
        .query_map(params![start, end, selected_project_path], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(|e| format!("ide split query failed: {e}"))?;
    for rr in rows {
        let (fam, c) = rr.map_err(|e| format!("ide split row failed: {e}"))?;
        ide_split.push((fam, c));
        total += c;
    }
    // Keep only the top two entries for display.
    ide_split.truncate(2);
    // Compute percentages proportional to the full total (all families),
    // then normalize the top-two shares so the displayed pair sums to 100.
    let mut ide_split_pct: Vec<(String, i64)> = Vec::new();
    if total > 0 && !ide_split.is_empty() {
        // Raw shares relative to the full total.
        let raw_shares: Vec<f64> = ide_split.iter().map(|(_, c)| (*c as f64) * 100.0 / (total as f64)).collect();
        // Sum of raw shares for the displayed entries.
        let sum_raw: f64 = raw_shares.iter().sum();
        if sum_raw > 0.0 {
            // Normalize each displayed share so they sum to 100, rounding first and leaving remainder to second.
            let first_norm = (raw_shares[0] / sum_raw) * 100.0;
            let p0 = first_norm.round() as i64;
            ide_split_pct.push((ide_split[0].0.clone(), p0));
            if ide_split.len() >= 2 {
                let p1 = 100 - p0;
                ide_split_pct.push((ide_split[1].0.clone(), p1));
            }
        } else {
            // No meaningful raw share (shouldn't happen), return zeros for entries.
            for (fam, _) in ide_split.iter() {
                ide_split_pct.push((fam.clone(), 0));
            }
        }
    } else {
        for (fam, _) in ide_split.into_iter() {
            ide_split_pct.push((fam, 0));
        }
    }

    let proj = projects
        .iter()
        .find(|p| p.project_path == selected_project_path)
        .cloned()
        .unwrap_or_else(|| projects[0].clone());

    let card2 = WrappedCard2 {
        project: proj,
        prompted,
        prompt_chars,
        agent_hours: (agent_hours_project * 10.0).round() / 10.0,
        ide_split: ide_split_pct,
    };

    // ---- Card3 metrics (simplified v1) ----
    let files_count: i64 = conn
        .query_row(
            r#"
SELECT COUNT(DISTINCT ef.path)
FROM event_files ef
JOIN events e ON e.id = ef.event_id
WHERE e.ts_s BETWEEN ?1 AND ?2
"#,
            params![start, end],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let sessions_count: i64 = prompts_count; // v1 proxy
    let avg_session_minutes = 0.0;

    // Night ratio + max parallel are simplified proxies.
    let night_ratio = 0.0;
    let max_parallel_agents = 1;

    let error_count: i64 = conn
        .query_row(
            r#"SELECT COUNT(*) FROM events WHERE ts_s BETWEEN ?1 AND ?2 AND state='error'"#,
            params![start, end],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let error_ratio = if events_count > 0 {
        (error_count as f64) / (events_count as f64)
    } else {
        0.0
    };

    let approvals_total: i64 = conn
        .query_row(
            r#"SELECT COUNT(*) FROM approvals WHERE created_at_s BETWEEN ?1 AND ?2"#,
            params![start, end],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let approvals_decided: i64 = conn
        .query_row(
            r#"SELECT COUNT(*) FROM approvals WHERE created_at_s BETWEEN ?1 AND ?2 AND status IN ('approved','denied')"#,
            params![start, end],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let approval_ratio = if approvals_total > 0 {
        approvals_decided as f64 / approvals_total as f64
    } else {
        0.0
    };

    let favourite_agent = conn
        .query_row(
            r#"SELECT agent_family FROM events WHERE ts_s BETWEEN ?1 AND ?2 GROUP BY agent_family ORDER BY COUNT(*) DESC LIMIT 1"#,
            params![start, end],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "Unknown".to_string());

    // Favourite model: omit entirely when absent.
    let favourite_model: Option<String> = conn
        .query_row(
            r#"SELECT model FROM events WHERE ts_s BETWEEN ?1 AND ?2 AND model IS NOT NULL GROUP BY model ORDER BY COUNT(*) DESC LIMIT 1"#,
            params![start, end],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .unwrap_or(None);

    let metrics = WrappedMetrics {
        agent_hours: card1.agent_hours,
        projects_count,
        files_count,
        sessions_count,
        prompts_count,
        avg_session_minutes,
        night_ratio,
        max_parallel_agents,
        error_ratio,
        approval_ratio,
        favourite_agent,
        favourite_model,
    };

    let period_key = match range {
        RangeKind::Today => format!("today-{}", start),
        RangeKind::Past7 => format!("7d-{}", start),
    };
    let archetype = choose_archetype(&metrics, &period_key);

    let card3 = WrappedCard3 {
        archetype,
        metrics,
    };

    Ok(WrappedOut {
        range: match range { RangeKind::Today => "today", RangeKind::Past7 => "past7" }.to_string(),
        start_ts_s: start,
        end_ts_s: end,
        card1,
        card2,
        card3,
        projects,
    })
}

// ---------- Archetype logic (deterministic) ----------

fn simple_hash(s: &str) -> i32 {
    // small deterministic hash (djb2)
    let mut h: u32 = 5381;
    for b in s.as_bytes() {
        h = (h.wrapping_shl(5)).wrapping_add(h) ^ (*b as u32);
    }
    h as i32
}

fn pick_description(name: &str, period_key: &str) -> String {
    let opts: &[&str] = match name {
        "The Swarm Lord" => &[
            "You run a squad of agents in parallel and let them build while you steer.",
            "You spin up a small army of agents and keep them flowing in sync.",
            "You orchestrate multiple agents at once without breaking a sweat.",
        ],
        "The Micromanager" => &[
            "You keep your agents on a tight leash and approve every move.",
            "You treat agents like interns and check all their work twice.",
            "You’re in the driver’s seat — your agents don’t get far without your say‑so.",
        ],
        "The Zen Architect" => &[
            "You give agents calm, clear briefs and let them ship clean changes.",
            "You design steady, focused sessions where agents rarely hit errors.",
            "You keep the architecture tidy and the agents in a peaceful flow.",
        ],
        "The Chaos Farmer" => &[
            "You unleash agents on wild problems and harvest whatever they grow.",
            "You spin up chaos and somehow manage to land the plane at the end.",
            "You let agents explore, fail, and try again until something sticks.",
        ],
        "The Night Coder" => &[
            "Your agents do their best work while everyone else is asleep.",
            "You and your agents keep the lights on long after midnight.",
            "Most of your agent time happens when the rest of the world is offline.",
        ],
        "The Sprinter" => &[
            "You fire up agents for short, sharp bursts of work.",
            "You like quick wins — lots of small sessions instead of long marathons.",
            "Your agents jump in, fix something, and move on.",
        ],
        "The Deep Diver" => &[
            "You dive deep with your agents on one big problem at a time.",
            "You prefer long, focused sessions with agents on a single project.",
            "You and your agents camp out in one codebase and explore it thoroughly.",
        ],
        "The Polyglot Builder" => &[
            "Your agents jump across projects and keep many things moving at once.",
            "You spread agent power across multiple codebases in the same period.",
            "Your week was a tour of different repos with agents helping everywhere.",
        ],
        "The Quiet Builder" => &[
            "You use agents sparingly and pick your moments.",
            "You’re still feeling out where agents fit into your workflow.",
            "When you bring agents in, it’s for specific, focused tasks.",
        ],
        _ => &[
            "You work alongside agents in a balanced, steady way.",
            "You and your agents share the workload without extremes.",
            "Your workflow blends human judgment and agent power smoothly.",
        ],
    };
    let idx = (simple_hash(&format!("{}{}", name, period_key)).abs() as usize) % opts.len();
    opts[idx].to_string()
}

pub fn choose_archetype(metrics: &WrappedMetrics, period_key: &str) -> ArchetypeOut {
    let name = if metrics.max_parallel_agents >= 3 && metrics.approval_ratio < 0.4 {
        "The Swarm Lord"
    } else if metrics.max_parallel_agents <= 1 && metrics.approval_ratio >= 0.8 && metrics.sessions_count >= 3 {
        "The Micromanager"
    } else if metrics.error_ratio < 0.05 && metrics.avg_session_minutes >= 25.0 {
        "The Zen Architect"
    } else if (metrics.error_ratio >= 0.15 || metrics.sessions_count >= 10) && metrics.files_count >= 20 {
        "The Chaos Farmer"
    } else if metrics.night_ratio >= 0.5 {
        "The Night Coder"
    } else if metrics.avg_session_minutes < 15.0 && metrics.sessions_count >= 5 {
        "The Sprinter"
    } else if metrics.avg_session_minutes >= 45.0 && metrics.projects_count == 1 {
        "The Deep Diver"
    } else if metrics.projects_count >= 3 && metrics.files_count >= 30 {
        "The Polyglot Builder"
    } else if metrics.agent_hours < 3.0 && metrics.sessions_count <= 3 {
        "The Quiet Builder"
    } else {
        "The Operator"
    };

    ArchetypeOut {
        archetype_name: name.to_string(),
        description: pick_description(name, period_key),
    }
}
