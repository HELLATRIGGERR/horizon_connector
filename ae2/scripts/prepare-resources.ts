import { existsSync, mkdirSync, symlinkSync, realpathSync } from 'node:fs';
import { join } from 'node:path';
import { engine, root, manifest, run, help } from './common';
if (!help('Usage: bun run ae2/scripts/prepare-resources.ts --minecraft-dir PATH --server-jar PATH\nRequires your installed Minecraft Java 26.3 and JDK 25. Never downloads Minecraft.')) {
  await run(['cargo', '+1.95.0', 'run', '--manifest-path', manifest, '--locked', '--release', '-p', 'horizon-prepare', '--', '--prepare', '--root', engine, ...process.argv.slice(2)], engine);
  const prepared = join(engine, '.local/horizon-client/26.3');
  if (!existsSync(prepared)) throw new Error(`Preparation did not produce ${prepared}`);
  const resources = join(root, '.local/resources');
  mkdirSync(resources, { recursive: true });
  const alias = join(resources, '26.3');
  if (!existsSync(alias)) symlinkSync(prepared, alias, 'dir');
  else if (realpathSync(alias) !== realpathSync(prepared)) throw new Error(`Existing ${alias} points elsewhere; select it with AE2_RESOURCES instead of replacing it.`);
}
