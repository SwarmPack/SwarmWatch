import { useMemo, useState } from 'react';
import type { WrappedOut, WrappedProjectOption, WrappedRange } from '../types';

type Props = {
  data: WrappedOut;
  range: WrappedRange;
  projects: WrappedProjectOption[];
  selectedProjectPath: string | null;
  onChangeRange: (r: WrappedRange) => void;
  onChangeProjectPath: (p: string) => void;
  onShare: (cardIndex: 0 | 1 | 2) => void;
  shareDisabled?: boolean;
};

function pctToText(v: number): string {
  if (!Number.isFinite(v)) return '0%';
  return `${Math.max(0, Math.min(100, Math.round(v)))}%`;
}

export function WrappedCard({
  data,
  range,
  projects,
  selectedProjectPath,
  onChangeRange,
  onChangeProjectPath,
  onShare,
  shareDisabled
}: Props) {
  const [cardIndex, setCardIndex] = useState<0 | 1 | 2>(0);
  const proj = data.card2?.project;

  const projectOptions = useMemo(() => {
    return (projects ?? []).filter((p) => p.project_path);
  }, [projects]);

  return (
    <div className="wrappedPanel">
      <div className="wrappedHeader">
        <div>
          <div className="settingsTitle">Agent Wrapped</div>
          <div className="settingsSub">Today or last 7 days, computed locally.</div>
        </div>
        <div className="wrappedHeaderRight">
          <div className="wrappedPager">
            <button
              type="button"
              className="wrappedPagerBtn"
              onClick={() => setCardIndex((v) => ((v + 2) % 3) as 0 | 1 | 2)}
              aria-label="Previous card"
            >
              {'<'}
            </button>
            <div className="wrappedPagerDots" aria-label="Card position">
              {[0, 1, 2].map((i) => (
                <span key={i} className={i === cardIndex ? 'dot on' : 'dot'} />
              ))}
            </div>
            <button
              type="button"
              className="wrappedPagerBtn"
              onClick={() => setCardIndex((v) => ((v + 1) % 3) as 0 | 1 | 2)}
              aria-label="Next card"
            >
              {'>'}
            </button>
          </div>
        </div>
      </div>

      <div className="wrappedControls">
        <div className="wrappedControl">
          <div className="wrappedLabel">Range</div>
          <div className="wrappedSeg">
            <button
              type="button"
              className={range === 'today' ? 'seg on' : 'seg'}
              onClick={() => onChangeRange('today')}
            >
              Today
            </button>
            <button
              type="button"
              className={range === 'past7' ? 'seg on' : 'seg'}
              onClick={() => onChangeRange('past7')}
            >
              Past 7
            </button>
          </div>
        </div>

        <div className="wrappedControl">
          <div className="wrappedLabel">Project</div>
          <select
            className="wrappedSelect"
            value={selectedProjectPath ?? ''}
            onChange={(e) => onChangeProjectPath(e.target.value)}
          >
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
      </div>

      <div className="wrappedCarousel" aria-label="Wrapped cards">
        {cardIndex === 0 ? (
          <div className="wrappedMiniCard">
            <div className="wrappedMiniHeader">
              <div className="wrappedMiniTitle">Card 1 · Summary</div>
              <button
                type="button"
                className="wrappedShareBtn"
                onClick={() => onShare(0)}
                disabled={shareDisabled}
                aria-label="Share card 1"
              >
                Share
              </button>
            </div>
            <div className="wrappedGrid">
              <div className="wrappedStat">
                <div className="k">Agent hours</div>
                <div className="v">{data.card1.agent_hours.toFixed(1)}</div>
              </div>
              <div className="wrappedStat">
                <div className="k">Projects</div>
                <div className="v">{data.card1.projects_count}</div>
              </div>
              <div className="wrappedStat">
                <div className="k">Thinking</div>
                <div className="v">{pctToText(data.card1.thinking_pct)}</div>
              </div>
              <div className="wrappedStat">
                <div className="k">Editing</div>
                <div className="v">{pctToText(data.card1.editing_pct)}</div>
              </div>
              <div className="wrappedStat wide">
                <div className="k">Running tools</div>
                <div className="v">{pctToText(data.card1.running_tools_pct)}</div>
              </div>
            </div>
          </div>
        ) : null}

        {cardIndex === 1 ? (
          <div className="wrappedMiniCard">
            <div className="wrappedMiniHeader">
              <div className="wrappedMiniTitle">Card 2 · {proj?.project_name ?? 'Project'}</div>
              <button
                type="button"
                className="wrappedShareBtn"
                onClick={() => onShare(1)}
                disabled={shareDisabled}
                aria-label="Share card 2"
              >
                Share
              </button>
            </div>
            <div className="wrappedGrid">
              <div className="wrappedStat">
                <div className="k">Prompted</div>
                <div className="v">{data.card2.prompted}</div>
              </div>
              <div className="wrappedStat">
                <div className="k">Prompt chars</div>
                <div className="v">{data.card2.prompt_chars}</div>
              </div>
              <div className="wrappedStat">
                <div className="k">Agent hours</div>
                <div className="v">{data.card2.agent_hours.toFixed(1)}</div>
              </div>
              <div className="wrappedStat wide">
                <div className="k">IDE split</div>
                <div className="v">
                  {(data.card2.ide_split ?? []).length
                    ? data.card2.ide_split.map(([fam, p]) => `${fam} ${p}%`).join(' · ')
                    : '-'}
                </div>
              </div>
            </div>
          </div>
        ) : null}

        {cardIndex === 2 ? (
          <div className="wrappedMiniCard">
            <div className="wrappedMiniHeader">
              <div className="wrappedMiniTitle">Card 3 · Archetype</div>
              <button
                type="button"
                className="wrappedShareBtn"
                onClick={() => onShare(2)}
                disabled={shareDisabled}
                aria-label="Share card 3"
              >
                Share
              </button>
            </div>
            <div className="wrappedArchetypeName">{data.card3.archetype.archetype_name}</div>
            <div className="wrappedArchetypeDesc">{data.card3.archetype.description}</div>

            <div className="wrappedGrid">
              <div className="wrappedStat">
                <div className="k">Files</div>
                <div className="v">{data.card3.metrics.files_count}</div>
              </div>
              <div className="wrappedStat">
                <div className="k">Errors</div>
                <div className="v">{pctToText(data.card3.metrics.error_ratio * 100)}</div>
              </div>
              <div className="wrappedStat">
                <div className="k">Approvals</div>
                <div className="v">{pctToText(data.card3.metrics.approval_ratio * 100)}</div>
              </div>
              <div className="wrappedStat wide">
                <div className="k">Favourite agent</div>
                <div className="v">{data.card3.metrics.favourite_agent}</div>
              </div>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
