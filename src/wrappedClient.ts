import type { WrappedOut, WrappedRange } from './types';

const BASE = 'http://127.0.0.1:4100';

type WrappedResponse =
  | { ok: true; data: WrappedOut }
  | { ok: false; error: string };

export async function fetchWrapped(args: {
  range: WrappedRange;
  projectPath?: string | null;
}): Promise<WrappedOut> {
  const url = new URL(`${BASE}/wrapped`);
  url.searchParams.set('range', args.range);
  if (args.projectPath) url.searchParams.set('project_path', args.projectPath);

  const res = await fetch(url.toString());
  const json = (await res.json().catch(() => null)) as WrappedResponse | null;
  if (!res.ok || !json) {
    throw new Error(`wrapped fetch failed (${res.status})`);
  }
  if (!json.ok) throw new Error(json.error || 'wrapped error');
  return json.data;
}
