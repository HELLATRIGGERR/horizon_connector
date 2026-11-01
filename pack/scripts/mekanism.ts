// Builds and tests the pack's Mekanism port in a checkout of this fork: the content mod from the ATM10 carry
// (mekanism-content), the game staged with the native bundle pack/mods/mekanism (horizon-mixins apply), its
// build, the crates' unit tests and the world tests.
// Usage: bun run pack/scripts/mekanism.ts <step>... (steps: content, units, stage, build, world; all: every one)
//
// Paths default to this machine's layout beside the checkout (`<base>` = the checkout's parent folder) and can be
// set in the environment:
//   MEK_CARRY        the ATM10 carry of Mekanism      <base>/ports/atm10/mods/mekanism/carry
//   MEK_CONTENT_DIR  the content mod it writes       <base>/.build/content/mekanism
//   MEK_STAGE        the staged, modded source       <base>/stage-mek
//   MEK_TARGET       the staged game's target dir    <base>/.build/target-mek-game
//   MEK_TOOL_TARGET  target dir of the pack's crates <base>/.build/target-mek
//   HORIZON_MIXINS   the horizon-mixins binary       <base>/.build/target-mixins/debug/horizon-mixins
//   MEK_JOBS         cargo jobs                      2
// `world` also passes ATM10_DIR (ATM10's settings), AE2_RESOURCES and AE2_CONTENT_DIR through to the harness.
// Building native code is the player's own decision; the launcher's `native-build` is the player's route.
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';

const root = resolve(import.meta.dir, '../..');
const base = resolve(root, '..');
const exe = process.platform === 'win32' ? '.exe' : '';
const bundle = join(root, 'pack/mods/mekanism');
const env = (name: string, fallback: string) => resolve(process.env[name] ?? fallback);
const carry = env('MEK_CARRY', join(base, 'ports/atm10/mods/mekanism/carry'));
const content = env('MEK_CONTENT_DIR', join(base, '.build/content/mekanism'));
const stage = env('MEK_STAGE', join(base, 'stage-mek'));
const target = env('MEK_TARGET', join(base, '.build/target-mek-game'));
const toolTarget = env('MEK_TOOL_TARGET', join(base, '.build/target-mek'));
const mixins = env('HORIZON_MIXINS', join(base, `.build/target-mixins/debug/horizon-mixins${exe}`));
const jobs = process.env.MEK_JOBS ?? '2';
// Small, low-memory builds: no incremental caches, no debug info.
const cargoEnv = { ...process.env, CARGO_INCREMENTAL: '0', CARGO_PROFILE_DEV_DEBUG: '0', CARGO_PROFILE_TEST_DEBUG: '0', CARGO_PROFILE_FAST_DEBUG: '0', CARGO_PROFILE_FAST_INCREMENTAL: 'false' };

async function run(command: string[], cwd = root, extra: Record<string, string | undefined> = cargoEnv): Promise<void> {
  console.log('>', command.join(' '));
  const child = Bun.spawn(command, { cwd, env: extra, stdin: 'inherit', stdout: 'inherit', stderr: 'inherit' });
  const code = await child.exited;
  if (code !== 0) throw new Error(`${command[0]} exited with status ${code}`);
}

/** A bundle crate may not hold a Cargo.lock (horizon-mixins refuses it); `units` makes one. */
function dropCrateLock(): void {
  rmSync(join(bundle, 'mekanism-energy/Cargo.lock'), { force: true });
}

/** Every file under `dir`, relative to it. */
function files(dir: string, under = dir, out: string[] = []): string[] {
  for (const entry of readdirSync(under, { withFileTypes: true })) {
    const path = join(under, entry.name);
    if (entry.isDirectory()) files(dir, path, out);
    else if (entry.isFile()) out.push(relative(dir, path));
  }
  return out;
}

/** Makes `to` hold what `from` holds, writing only the files that differ, so Cargo (which goes by file times)
 * rebuilds only the crates the bundle touched, as the launcher does with its `native/stage/`. */
function syncStage(from: string, to: string): number {
  const fresh = new Set(files(from));
  let written = 0;
  for (const file of fresh) {
    const source = join(from, file), dest = join(to, file);
    if (existsSync(dest) && readFileSync(dest).equals(readFileSync(source))) continue;
    mkdirSync(dirname(dest), { recursive: true });
    copyFileSync(source, dest);
    written++;
  }
  if (existsSync(to)) for (const file of files(to)) if (!fresh.has(file)) { rmSync(join(to, file)); written++; }
  return written;
}

const steps: Record<string, () => Promise<void>> = {
  async content() {
    await run(['cargo', 'run', '--offline', '-q', '-j', jobs, '--manifest-path', join(bundle, 'mekanism-content/Cargo.toml'), '--target-dir', toolTarget,
      '--', '--carry', carry, '--overlay', join(bundle, 'content'), '--out', content]);
  },
  async units() {
    try {
      for (const crate of ['mekanism-energy', 'mekanism-content']) {
        await run(['cargo', 'test', '--offline', '-j', jobs, '--manifest-path', join(bundle, crate, 'Cargo.toml'), '--target-dir', toolTarget]);
      }
    } finally { dropCrateLock(); }
  },
  async stage() {
    dropCrateLock();
    // horizon-mixins replaces an existing stage only on Linux: apply into a fresh folder, then copy what changed.
    const next = `${stage}-next`;
    rmSync(next, { recursive: true, force: true });
    await run([mixins, 'apply', '--workspace', root, '--mods', join(root, 'pack/mods'), '--out', next]);
    console.log(`${syncStage(next, stage)} file(s) of ${stage} changed`);
    rmSync(next, { recursive: true, force: true });
  },
  async build() {
    await run(['cargo', 'build', '--locked', '--profile', 'fast', '-p', 'horizon-client', '--no-default-features', '--features', 'ae2-lab', '--bins', '-j', jobs, '--target-dir', target], stage);
  },
  async world() {
    const binary = join(target, `fast/horizon-client${exe}`);
    if (!existsSync(binary)) throw new Error(`No staged build at ${binary}; run the build step.`);
    // A worktree has neither the prepared resources nor AE2's content: the main checkout's (`<base>/horizon`).
    const prepared = (path: string) => existsSync(join(root, path)) ? join(root, path) : join(base, 'horizon', path);
    const harness = {
      ...process.env, AE2_TEST_BINARY: binary, MEK_CONTENT_DIR: content,
      AE2_RESOURCES: process.env.AE2_RESOURCES ?? prepared('.local/resources/26.3'),
      AE2_CONTENT_DIR: process.env.AE2_CONTENT_DIR ?? prepared('content/ae2'),
      AE2_TEST_STARTUP_MS: process.env.AE2_TEST_STARTUP_MS ?? '600000',
    };
    // MEK_TESTS picks some (`energy-cube-screen`); all of pack/mods/mekanism/tests by default.
    const names = process.env.MEK_TESTS?.split(',').filter(Boolean)
      ?? readdirSync(join(bundle, 'tests')).filter(f => f.endsWith('.ts')).map(f => f.slice(0, -3)).sort();
    for (const name of names) await run(['bun', 'run', join(bundle, 'tests', `${name}.ts`)], root, harness);
  },
};

const args = process.argv.slice(2);
if (args.length === 0 || args.includes('--help') || args.includes('-h')) {
  console.log('Usage: bun run pack/scripts/mekanism.ts <step>...\nSteps, in order: ' + Object.keys(steps).join(', ') + '; all runs every one.');
  process.exit(0);
}
for (const step of args.includes('all') ? Object.keys(steps) : args) {
  const go = steps[step];
  if (!go) throw new Error(`Unknown step ${step}; steps: ${Object.keys(steps).join(', ')}`);
  await go();
}
