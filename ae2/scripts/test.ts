import { join } from 'node:path';
import { existsSync } from 'node:fs';
import { root, engine, manifest, run, help, packFeatures } from './common';
if (!help('Usage: bun run ae2/scripts/test.ts [--client]\nRuns the five native core test suites. --client also runs client AE2 library and host tests, with the features of the mods pack/pack.toml turns on. Opens no game window.')) {
  const args = process.argv.slice(2);
  if (args.some(arg => arg !== '--client')) throw new Error('Only --client is accepted.');
  for (const name of ['cell', 'crafting', 'links', 'machines', 'network']) {
    const crate = join(root, `crates/ae2-${name}-core`);
    await run(['cargo', 'test', '--locked', '--manifest-path', join(crate, 'Cargo.toml')], crate);
  }
  if (args.includes('--client')) {
    if (!existsSync(join(root, '.local/resources/26.3/items.json'))) {
      throw new Error('The client host tests need prepared resources. Run ae2/scripts/prepare-resources.ts first.');
    }
    const guide = join(root, 'content/ae2/assets/ae2/ae2guide');
    const env = { ...process.env };
    if (existsSync(join(guide, 'compiled-guide.json'))) env.AE2_GUIDE_TEST_ROOT = guide;
    else console.log('Guide corpus is absent; that environment-dependent test skips its corpus. Run prepare-content.ts first to include it.');
    for (const target of ['--lib', '--bin']) {
      await run(['cargo', 'test', '--locked', '--manifest-path', manifest, '--profile', 'fast', '-p', 'horizon-client', '--no-default-features', '--features', packFeatures(), target, ...(target === '--bin' ? ['horizon-client'] : []), 'ae2'], engine, env);
    }
  }
}
