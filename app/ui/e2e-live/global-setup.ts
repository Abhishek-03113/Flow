import { execFileSync } from 'node:child_process';
import { HARNESS_BIN, REPO } from './harness';

/**
 * Makes sure `flow-harness` is built (release). Cargo reuses an up-to-date
 * binary, so this costs a second when nothing changed. `FLOW_HARNESS_BIN`
 * skips the build and uses that binary as is.
 */
export default function globalSetup(): void {
  if (process.env.FLOW_HARNESS_BIN) return;
  console.log(`building ${HARNESS_BIN} (cargo build -p flow-harness --release)`);
  execFileSync('cargo', ['build', '-p', 'flow-harness', '--release'], { cwd: REPO, stdio: 'inherit' });
}
