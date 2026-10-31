import { existsSync, mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { root, engine, run, help } from './common';
if (!help('Usage: bun run ae2/scripts/play.ts [Horizon client arguments]\nInteractive launcher: opens a visible client with saves/settings isolated under .local/play/.\nFor a new creative world pass --world NAME --creative. Automated fixtures need their own background harness.')) {
  if (process.argv.slice(2).some(arg => arg === '--background' || arg === '--offscreen')) throw new Error('This is the interactive launcher. Automated fixtures require an isolated background harness with a rendering lease and 30 FPS cap.');
  const binary = resolve(process.env.AE2_CLIENT ?? join(engine, 'target/fast', process.platform === 'win32' ? 'horizon-client.exe' : 'horizon-client'));
  const resources = resolve(process.env.AE2_RESOURCES ?? join(root, '.local/resources/26.3'));
  const content = resolve(process.env.AE2_CONTENT_DIR ?? join(root, 'content/ae2'));
  for (const path of [binary, resources, content]) if (!existsSync(path)) throw new Error(`Missing ${path}; follow the README build/preparation steps.`);
  const profile = join(root, '.local/play');
  const game = join(profile, 'game');
  mkdirSync(game, { recursive: true });
  await run([binary, '--resources', resources, '--dev-mod', content, '--server-thread', ...process.argv.slice(2)], game, {
    ...process.env,
    HORIZON_HOME: game,
    HORIZON_LAUNCHER_HOME: join(profile, 'launcher'),
    XDG_DATA_HOME: join(profile, 'data'),
    XDG_CONFIG_HOME: join(profile, 'config'),
    HORIZON_LAUNCHER_NO_SELF_UPDATE: '1',
    HORIZON_TELEMETRY_URL: 'off',
    HORIZON_SERVER_THREAD: '1',
  });
}
