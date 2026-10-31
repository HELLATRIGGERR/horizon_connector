/** A host-wide rendering lease. No child process inherits the lease descriptor. */
import { dlopen, ptr } from 'bun:ffi';
import { closeSync, ftruncateSync, openSync, readFileSync, rmSync, writeSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

export async function acquireRenderLease(): Promise<() => void> {
  if (process.platform === 'win32') return acquireWindowsLease();
  if (process.platform !== 'darwin' && process.platform !== 'linux') throw new Error('The hidden world harness supports macOS, Linux and Windows only.');
  const libc = dlopen(process.platform === 'darwin' ? '/usr/lib/libSystem.B.dylib' : 'libc.so.6', {
    flock: { args: ['i32', 'i32'], returns: 'i32' },
    fcntl: { args: ['i32', 'i32', 'i32'], returns: 'i32' },
  });
  const fd = openSync(join(tmpdir(), `ae2-horizon-render-${process.getuid?.() ?? 'user'}.lock`), 'a+', 0o600);
  if (libc.symbols.fcntl(fd, 2, 1) !== 0) { closeSync(fd); libc.close(); throw new Error('Cannot make the render lease close on exec.'); }
  try {
    while (libc.symbols.flock(fd, 6) !== 0) await Bun.sleep(250);
    ftruncateSync(fd, 0);
    writeSync(fd, JSON.stringify({ pid: process.pid, acquired_at: new Date().toISOString() }) + '\n');
  } catch (error) { closeSync(fd); libc.close(); throw error; }
  let released = false;
  return () => { if (!released) { released = true; closeSync(fd); libc.close(); } };
}

// Windows has no flock: the lease is a file created exclusively, taken over
// only when the process that holds it is gone.
async function acquireWindowsLease(): Promise<() => void> {
  const path = join(tmpdir(), `ae2-horizon-render-${process.env.USERNAME ?? 'user'}.lock`);
  for (;;) {
    try {
      const fd = openSync(path, 'wx', 0o600);
      writeSync(fd, JSON.stringify({ pid: process.pid, acquired_at: new Date().toISOString() }) + '\n');
      closeSync(fd);
      let released = false;
      return () => { if (!released) { released = true; rmSync(path, { force: true }); } };
    } catch (error: any) {
      if (error?.code !== 'EEXIST') throw error;
      let holder = 0;
      try { holder = JSON.parse(readFileSync(path, 'utf8')).pid; } catch {}
      let alive = false;
      if (holder) try { process.kill(holder, 0); alive = true; } catch {}
      if (!alive) rmSync(path, { force: true });
      else await Bun.sleep(250);
    }
  }
}

// Match Rust Instant on Darwin: mach_absolute_time excludes system sleep.
// A closed lid may pause a test, but must not consume its active-time budget.
const awakeClock = (() => {
  if (process.platform !== 'darwin') return () => performance.now();
  const lib = dlopen('/usr/lib/libSystem.B.dylib', {
    mach_absolute_time: { args: [], returns: 'u64' },
    mach_timebase_info: { args: ['ptr'], returns: 'i32' },
  });
  const info = new Uint32Array(2);
  if (lib.symbols.mach_timebase_info(ptr(info)) !== 0 || !info[1]) throw new Error('Cannot read Darwin awake clock');
  const millisecondsPerTick = info[0] / info[1] / 1e6;
  return () => Number(lib.symbols.mach_absolute_time()) * millisecondsPerTick;
})();
export function awakeNowMs(): number { return awakeClock(); }
export function awakeTimeout(callback: () => void, milliseconds: number): () => void {
  const end = awakeNowMs() + milliseconds;
  let active = true;
  const timer = setInterval(() => {
    if (active && awakeNowMs() >= end) { active = false; clearInterval(timer); callback(); }
  }, Math.min(250, milliseconds));
  return () => { active = false; clearInterval(timer); };
}
