import { type ReactNode, useMemo, useState } from 'react';
import type { WrappedOut, WrappedProjectOption, WrappedRange } from '../types';

const SUMMARY_BAR_COLORS = ['#7ff0d6', '#f7b2ff'];
const PROJECT_BAR_COLORS = ['#ffd783', '#9ed7ff'];

const CARD_THEMES = [
  {
    key: 'summary',
    label: 'Summary',
    className: 'theme-summary',
    tagline: 'Your pack, at a glance'
  },
  {
    key: 'project',
    label: 'Project Specific',
    className: 'theme-project',
    tagline: 'Where the pack spent its time'
  },
  {
    key: 'archetype',
    label: 'Archetype',
    className: 'theme-archetype',
    tagline: 'Who you were this run'
  }
] as const;

type Props = {
  data: WrappedOut;
  range: WrappedRange;
  projects: WrappedProjectOption[];
  selectedProjectPath: string | null;
  onChangeRange: (r: WrappedRange) => void;
  onChangeProjectPath: (p: string) => void;
};

type PosterProps = {
  data: WrappedOut;
  cardIndex: 0 | 1 | 2;
  range: WrappedRange;
  variant?: 'live' | 'share';
};

function pctToText(v: number): string {
  if (!Number.isFinite(v)) return '0%';
  return `${Math.max(0, Math.min(100, Math.round(v)))}%`;
}

// Removed unused duration formatter from previous iteration

function rangeLabel(range: WrappedRange): string {
  return range === 'today' ? 'Today' : 'Past 7 days';
}

export function WrappedPosterCardView({ data, cardIndex, range, variant = 'live' }: PosterProps) {
  const theme = CARD_THEMES[cardIndex];
  const summary = data.card1;
  const project = data.card2;
  const archetype = data.card3;
  const isShare = variant === 'share';

  const summaryTopMetrics = [
    { label: 'Thinking', value: summary.thinking_pct },
    { label: 'Editing', value: summary.editing_pct },
    { label: 'Running', value: summary.running_tools_pct }
  ]
    .map((item) => {
        const pct = typeof item.value === 'number' ? item.value : Number(item.value);
        let value = Number.isFinite(pct) ? pct : 0;
        // Accept both 0-1 ratios and 0-100 percentages.
        if (value > 0 && value <= 1) value = value * 100;
        return { ...item, value };
    })
    .sort((a, b) => b.value - a.value)
    .slice(0, 2)
    .map((item, idx) => ({ ...item, color: SUMMARY_BAR_COLORS[idx % SUMMARY_BAR_COLORS.length] }));

  const projectTopSplit = (project.ide_split ?? [])
    .map(([fam, pct]) => {
        const pctNum = typeof pct === 'number' ? pct : Number(pct);
        let value = Number.isFinite(pctNum) ? pctNum : 0;
        if (value > 0 && value <= 1) value = value * 100;
        return {
          label: fam,
          value
        };
    })
    .sort((a, b) => b.value - a.value)
    .slice(0, 2)
    .map((item, idx) => ({ ...item, color: PROJECT_BAR_COLORS[idx % PROJECT_BAR_COLORS.length] }));

  if (!isShare) {
    let body: ReactNode = null;

    if (cardIndex === 0) {
      body = (
        <>
          <div className="posterMiniHero">
            {summary.agent_hours.toFixed(1)}
            <span>hrs</span>
          </div>
          <div className="posterMiniStats">
            <span>
              Projects<strong>{summary.projects_count}</strong>
            </span>
            <span>
              Prompted<strong>{data.card3.metrics.prompts_count} times</strong>
            </span>
          </div>
          <div className="posterMiniBars">
            {summaryTopMetrics.map((item) => (
              <div key={item.label}>
                <div className="posterMiniBarLabel">
                  <span>{item.label}</span>
                  <span>{pctToText(item.value)}</span>
                </div>
                <div className="posterMiniTrack">
                  <div
                    className="posterMiniFill"
                    style={{ width: `${Math.max(0, Math.min(100, item.value))}%`, background: item.color }}
                  />
                </div>
              </div>
            ))}
          </div>
        </>
      );
    } else if (cardIndex === 1) {
      body = (
        <>
          <div className="posterMiniTitle">{project.project?.project_name ?? 'Project focus'}</div>
          <div className="posterMiniHero">
            {project.agent_hours.toFixed(1)}
            <span>hrs</span>
          </div>
          <div className="posterMiniStats">
            <span>
              Prompted<strong>{project.prompted} times</strong>
            </span>
            <span>
              Prompt characters<strong>{project.prompt_chars}</strong>
            </span>
            {/* <span>
              Repo hrs<strong>{project.project?.agent_hours?.toFixed(1) ?? '—'}</strong>
            </span> */}
          </div>
          {projectTopSplit.length ? (
            <div className="posterMiniBars">
              {projectTopSplit.map((entry) => (
                <div key={entry.label}>
                  <div className="posterMiniBarLabel">
                    <span>{entry.label}</span>
                    <span>{pctToText(entry.value)}</span>
                  </div>
                  <div className="posterMiniTrack">
                    <div
                      className="posterMiniFill"
                      style={{ width: `${Math.max(0, Math.min(100, entry.value))}%`, background: entry.color }}
                    />
                  </div>
                </div>
              ))}
            </div>
          ) : null}
        </>
      );
    } else {
      body = (
        <>
          <div className="posterMiniTitle">{archetype.archetype.archetype_name}</div>
          <div className="posterMiniCaption">{archetype.archetype.description}</div>
          <div className="posterMiniStats">
            <span>
              Agents worked<strong>{data.card1.agent_hours.toFixed(1)} hours</strong>
            </span>
            <span>
              Favourite agent<strong>{archetype.metrics.favourite_agent}</strong>
            </span>
            <span>
              Files touched<strong>{archetype.metrics.files_count}</strong>
            </span>
            <span>
              Prompted<strong>{archetype.metrics.prompts_count} times</strong>
            </span>
          </div>
        </>
      );
    }

    return (
      <div className={`posterMini ${theme.className}`} aria-live="polite">
        <div className="posterMiniHeader">
          <span className="posterMiniLabel">{theme.label}</span>
          <span className="posterMiniRange">{rangeLabel(range)}</span>
        </div>
        {body}
        <div className="posterMiniBrand">SwarmWatch</div>
      </div>
    );
  }

  return (
    <div className={`posterCard ${theme.className} share`} aria-live="polite">
      <div className="posterHeader">
        <div>
          <div className="posterBadge">SwarmWatch · {rangeLabel(range)}</div>
          <div className="posterTagline">{theme.tagline}</div>
        </div>
        <div className="posterMeta">{theme.label}</div>
      </div>

      {cardIndex === 0 ? (
        <>
          <div className="posterHero">
            <div className="posterHeroNumber">
              {summary.agent_hours.toFixed(1)}
              <span>hrs</span>
            </div>
            <div className="posterHeroCaption">agents babysat this {rangeLabel(range).toLowerCase()}</div>
          </div>
          <div className="posterStatGrid">
            <div className="posterStatCard">
              <div>Projects watched</div>
              <strong>{summary.projects_count}</strong>
            </div>
            <div className="posterStatCard">
              <div>Prompts</div>
              <strong>{data.card3.metrics.prompts_count}</strong>
            </div>
          </div>
          <div className="posterBarGroup">
            {summaryTopMetrics.map((item) => (
              <div key={item.label} className="posterBar">
                <div className="posterBarLabel">
                  <span>{item.label}</span>
                  <span>{pctToText(item.value)}</span>
                </div>
                <div className="posterBarTrack">
                  <div
                    className="posterBarFill"
                    style={{ width: `${Math.max(0, Math.min(100, item.value))}%`, background: item.color }}
                  />
                </div>
              </div>
            ))}
          </div>
        </>
      ) : cardIndex === 1 ? (
        <>
          <div className="posterHero">
            <div className="posterHeroTitle">{project.project?.project_name ?? 'Project focus'}</div>
            <div className="posterHeroNumber">
              {project.agent_hours.toFixed(1)}
              <span>hrs</span>
            </div>
            <div className="posterHeroCaption">spent inside this repo</div>
          </div>
          <div className="posterPillRow">
            <div className="posterChip">
              <span>Prompts</span>
              <strong>{project.prompted}</strong>
            </div>
            <div className="posterChip">
              <span>Prompt chars</span>
              <strong>{project.prompt_chars}</strong>
            </div>
          </div>
          <div className="posterStatGrid">
            <div className="posterStatCard">
              <div>Repo hours</div>
              <strong>{project.project?.agent_hours?.toFixed(1) ?? '—'}</strong>
            </div>
            <div className="posterStatCard">
              <div>Prompt chars</div>
              <strong>{project.prompt_chars}</strong>
            </div>
            <div className="posterStatCard span2">
              <div>IDE split</div>
              {projectTopSplit.length ? (
                <div className="posterBarGroup">
                  {projectTopSplit.map((entry) => (
                    <div key={entry.label} className="posterBar">
                      <div className="posterBarLabel">
                        <span>{entry.label}</span>
                        <span>{pctToText(entry.value)}</span>
                      </div>
                      <div className="posterBarTrack">
                        <div
                          className="posterBarFill"
                          style={{ width: `${Math.max(0, Math.min(100, entry.value))}%`, background: entry.color }}
                        />
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="posterSplit">—</div>
              )}
            </div>
            <div className="posterStatCard span2">
              <div>Project path</div>
              <strong>{project.project?.project_path ?? '—'}</strong>
            </div>
          </div>
        </>
      ) : (
        <>
          <div className="posterHero">
            <div className="posterHeroTitle">{archetype.archetype.archetype_name}</div>
            <div className="posterHeroCaption">{archetype.archetype.description}</div>
          </div>
          <div className="posterPillRow">
            <div className="posterChip">
              <span>Favourite agent</span>
              <strong>{archetype.metrics.favourite_agent}</strong>
            </div>
          </div>
          <div className="posterStatGrid">
            <div className="posterStatCard">
              <div>Agents worked</div>
              <strong>{data.card1.agent_hours.toFixed(1)}</strong>
            </div>
            <div className="posterStatCard">
              <div>Fav agent</div>
              <strong>{archetype.metrics.favourite_agent}</strong>
            </div>
            <div className="posterStatCard">
              <div>Files touched</div>
              <strong>{archetype.metrics.files_count}</strong>
            </div>
            <div className="posterStatCard">
              <div>Prompts</div>
              <strong>{archetype.metrics.prompts_count}</strong>
            </div>
          </div>
        </>
      )}
      <div className="posterFooterLink">Visit: github.com/SwarmPack/SwarmWatch</div>
    </div>
  );
}

export function WrappedCard({
  data,
  range,
  projects,
  selectedProjectPath,
  onChangeRange,
  onChangeProjectPath,
}: Props) {
  const [cardIndex, setCardIndex] = useState<0 | 1 | 2>(0);

  const projectOptions = useMemo(() => {
    const seen = new Set<string>();
    return (projects ?? []).filter((p) => {
      if (!p.project_path) return false;
      if (seen.has(p.project_path)) return false;
      seen.add(p.project_path);
      return true;
    });
  }, [projects]);

  // theme selected by cardIndex is used directly in child view

  return (
    <div className="wrappedPanel">
      <div className="wrappedHeader posterHeaderBar">
        <div style={{ width: '100%' }}>
          <div className="settingsTitle">SwarmWatch — Agent Recap</div>
        </div>
      </div>

      <div className="wrappedControls">
        <div className="wrappedControl">
          <div className="wrappedSeg">
            <button type="button" className={range === 'today' ? 'seg on' : 'seg'} onClick={() => onChangeRange('today')}>
              Today
            </button>
            <button type="button" className={range === 'past7' ? 'seg on' : 'seg'} onClick={() => onChangeRange('past7')}>
              Past 7
            </button>
          </div>
        </div>

        {cardIndex === 1 ? (
          <div className="wrappedControl">
            {/* <div className="wrappedLabel">Project</div> */}
            <select className="wrappedSelect" value={selectedProjectPath ?? ''} onChange={(e) => onChangeProjectPath(e.target.value)}>
              {projectOptions.length ? (
                projectOptions.map((p) => (
                  <option key={p.project_path} value={p.project_path}>
                    {p.project_name}
                  </option>
                ))
              ) : (
                <option value="">(no project)</option>
              )}
            </select>
          </div>
        ) : null}
      </div>

      <div className="posterPreviewWrap">
        <button
          type="button"
          className="posterSideBtn left"
          aria-label="Previous card"
          onClick={() => setCardIndex((v) => ((v + 2) % 3) as 0 | 1 | 2)}
        >
          ‹
        </button>
        <div className="posterPreview" aria-label="Poster card preview">
          <WrappedPosterCardView data={data} cardIndex={cardIndex} range={range} />
        </div>
        <button
          type="button"
          className="posterSideBtn right"
          aria-label="Next card"
          onClick={() => setCardIndex((v) => ((v + 1) % 3) as 0 | 1 | 2)}
        >
          ›
        </button>
      </div>

      {/* share button removed per request */}
    </div>
  );
}
