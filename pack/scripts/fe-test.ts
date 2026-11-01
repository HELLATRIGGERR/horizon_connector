// Tests the engine's Forge Energy capability (hook points API 63) with Mekanism's energy cubes and AE2: the
// `mekanism` bundle with the test bundle pack/tests/mekanism-fe over it (the cubes' FE edge), staged with
// horizon-mixins, built, and the world test pack/tests/mekanism-fe/fe-cube-ae2.ts run on that build.
// Usage: bun run pack/scripts/fe-test.ts <step>... (steps: units, stage, build, world; all: every one)
//
// The Mekanism content mod comes from `bun run pack/scripts/mekanism.ts content`. Paths default to this
// machine's layout beside the checkout (`<base>` = the checkout's parent folder) and can be set in the environment:
//   MEK_CONTENT_DIR  the Mekanism content mod        <base>/.build/content/mekanism
//   FE_STAGE         the staged, modded source       <base>/stage-fe
//   FE_TARGET        the staged game's target dir    <base>/.build/target-fe-game
//   HORIZON_MIXINS   the horizon-mixins binary       <base>/.build/target-mixins/debug/horizon-mixins
//   FE_JOBS          cargo jobs                      2
// `world` also passes AE2_RESOURCES and AE2_CONTENT_DIR through to the harness. The cubes' states need the
// engine's acceptance of a content block's template-first property order (`horizon-world` level/states.rs);
// without it the world refuses to load the content mod.
import { copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';

const root = resolve(import.meta.dir, '../..');
const base = resolve(root, '..');
const exe = process.platform === 'win32' ? '.exe' : '';
const bundles = { mekanism: join(root, 'pack/mods/mekanism'), 'mekanism-fe': join(root, 'pack/tests/mekanism-fe') };
const env = (name: string, fallback: string) => resolve(process.env[name] ?? fallback);
const content = env('MEK_CONTENT_DIR', join(base, '.build/content/mekanism'));
const stage = env('FE_STAGE', join(base, 'stage-fe'));
const target = env('FE_TARGET', join(base, '.build/target-fe-game'));
const mixins = env('HORIZON_MIXINS', join(base, `.build/target-mixins/debug/horizon-mixins${exe}`));
const jobs = process.env.FE_JOBS ?? '2';
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
  rmSync(join(bundles.mekanism, 'mekanism-energy/Cargo.lock'), { force: true });
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

/** Makes `to` hold what `from` holds, writing only the files that differ, so Cargo rebuilds only what changed. */
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
  async units() {
    try {
      await run(['cargo', 'test', '--offline', '-j', jobs, '--manifest-path', join(bundles.mekanism, 'mekanism-energy/Cargo.toml'), '--target-dir', target]);
    } finally { dropCrateLock(); }
  },
  async stage() {
    dropCrateLock();
    // horizon-mixins takes one folder of bundles: both, side by side, outside the checkout.
    const mods = `${stage}-mods`, next = `${stage}-next`;
    for (const dir of [mods, next]) rmSync(dir, { recursive: true, force: true });
    for (const [name, dir] of Object.entries(bundles)) {
      cpSync(dir, join(mods, name), { recursive: true });
      // A pinned `horizon_revision` would refuse this branch's commit; the hook catalog is what binds.
      const manifest = join(mods, name, 'horizon-mixin.toml');
      writeFileSync(manifest, readFileSync(manifest, 'utf8').replace(/^\[compatibility\]\n(?:[^[\n].*\n|\n)*/m, ''));
    }
    await run([mixins, 'apply', '--workspace', root, '--mods', mods, '--out', next]);
    console.log(`${syncStage(next, stage)} file(s) of ${stage} changed`);
    for (const dir of [mods, next]) rmSync(dir, { recursive: true, force: true });
  },
  async build() {
    await run(['cargo', 'build', '--locked', '--profile', 'fast', '-p', 'horizon-client', '--no-default-features', '--features', 'ae2-lab', '--bins', '-j', jobs, '--target-dir', target], stage);
  },
  async world() {
    const binary = join(target, `fast/horizon-client${exe}`);
    if (!existsSync(binary)) throw new Error(`No staged build at ${binary}; run the build step.`);
    if (!existsSync(content)) throw new Error(`No Mekanism content at ${content}; run bun run pack/scripts/mekanism.ts content.`);
    // A worktree has neither the prepared resources nor AE2's content: the main checkout's (`<base>/horizon`).
    const prepared = (path: string) => existsSync(join(root, path)) ? join(root, path) : join(base, 'horizon', path);
    const harness = {
      ...process.env, AE2_TEST_BINARY: binary, MEK_CONTENT_DIR: content,
      AE2_RESOURCES: process.env.AE2_RESOURCES ?? prepared('.local/resources/26.3'),
      AE2_CONTENT_DIR: process.env.AE2_CONTENT_DIR ?? prepared('content/ae2'),
      AE2_TEST_STARTUP_MS: process.env.AE2_TEST_STARTUP_MS ?? '600000',
    };
    await run(['bun', 'run', join(bundles['mekanism-fe'], 'fe-cube-ae2.ts')], root, harness);
  },
};

const args = process.argv.slice(2);
if (args.length === 0 || args.includes('--help') || args.includes('-h')) {
  console.log('Usage: bun run pack/scripts/fe-test.ts <step>...\nSteps, in order: ' + Object.keys(steps).join(', ') + '; all runs every one.');
  process.exit(0);
}
for (const step of args.includes('all') ? Object.keys(steps) : args) {
  const go = steps[step];
  if (!go) throw new Error(`Unknown step ${step}; steps: ${Object.keys(steps).join(', ')}`);
  await go();
}
