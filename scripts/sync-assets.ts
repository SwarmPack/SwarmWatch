import fs from 'node:fs/promises';
import path from 'node:path';

const ROOT = process.cwd();
const PUBLIC_DIR = path.join(ROOT, 'public');

// Keep this resilient across asset renames.
// Historically the repo used `completed.json`, while the UI uses `/done.json`.
const ASSETS: Array<{ src: string; dst: string }> = [
  { src: 'base3-idle.json', dst: 'base3-idle.json' },
  { src: 'exp-reading.json', dst: 'exp-reading.json' },
  { src: 'exp-editing.json', dst: 'exp-editing.json' },
  { src: 'base4-running.json', dst: 'base4-running.json' },
  { src: 'HelpNeed.json', dst: 'HelpNeed.json' },
  // Prefer done.json if present; otherwise fall back to completed.json but copy as done.json.
  { src: 'done.json', dst: 'done.json' },
  { src: 'completed.json', dst: 'done.json' },
  { src: 'error.json', dst: 'error.json' }
];

async function exists(p: string) {
  try {
    await fs.access(p);
    return true;
  } catch {
    return false;
  }
}

async function main() {
  await fs.mkdir(PUBLIC_DIR, { recursive: true });

  const usedDst = new Set<string>();
  for (const { src, dst } of ASSETS) {
    if (usedDst.has(dst)) continue;
    const srcPath = path.join(ROOT, src);
    if (!(await exists(srcPath))) continue;
    const dstPath = path.join(PUBLIC_DIR, dst);
    await fs.copyFile(srcPath, dstPath);
    usedDst.add(dst);
  }

  // Ensure done.json exists (either from done.json or completed.json).
  if (!(await exists(path.join(PUBLIC_DIR, 'done.json')))) {
    throw new Error('Missing done.json in public/ (expected done.json or completed.json at repo root)');
  }
}

main().catch((err) => {
  console.error('[sync-assets] failed:', err);
  process.exitCode = 1;
});
