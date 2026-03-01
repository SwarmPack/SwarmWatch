 import { execSync } from 'node:child_process';

const ports = [1420, 4100];

function run(cmd: string) {
  try {
    execSync(cmd, { stdio: 'ignore' });
  } catch {
    // ignore
  }
}

for (const port of ports) {
  // macOS/Linux: lsof to find PIDs listening on a port.
  // (No-op if lsof not available.)
  try {
    const out = execSync(`lsof -tiTCP:${port} -sTCP:LISTEN`, { encoding: 'utf8' })
      .split(/\s+/)
      .map((s) => s.trim())
      .filter(Boolean);

    for (const pid of out) {
      run(`kill ${pid}`);
    }
  } catch {
    // ignore
  }
}
