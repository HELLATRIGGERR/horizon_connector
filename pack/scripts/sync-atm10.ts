// Copies the enabled mods' settings from an installed ATM10 into the game's config/,
// as pack/pack.toml lists them.
// Usage: ATM10_DIR="<ATM10 instance>" bun run pack/scripts/sync-atm10.ts [--home DIR]
import { copyFileSync, existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

const root = resolve(import.meta.dir, '../..');
const args = process.argv.slice(2);
if (args.includes('--help') || args.includes('-h')) {
  console.log('Usage: ATM10_DIR="<ATM10 instance>" bun run pack/scripts/sync-atm10.ts [--home DIR]\n'
    + 'Copies the enabled mods\' ATM10 configs into DIR/config (default: .local/play/game, where ae2/scripts/play.ts runs the game).');
  process.exit(0);
}
const homeAt = args.indexOf('--home');
const home = resolve(homeAt >= 0 ? args[homeAt + 1] : join(root, '.local/play/game'));
const atm10 = process.env.ATM10_DIR;
if (!atm10 || !existsSync(join(atm10, 'config'))) {
  throw new Error('Set ATM10_DIR to an ATM10 instance (the folder that holds its config/).');
}

const pack = Bun.TOML.parse(readFileSync(join(root, 'pack/pack.toml'), 'utf8')) as {
  mods: Record<string, { name: string; enabled: boolean; configs?: string[] }>;
};
let copied = 0;
for (const [id, mod] of Object.entries(pack.mods)) {
  if (!mod.enabled) continue;
  for (const file of mod.configs ?? []) {
    const from = join(atm10, 'config', file);
    if (!existsSync(from)) {
      console.warn(`${id}: ATM10 has no config/${file}; the mod keeps its defaults there.`);
      continue;
    }
    const to = join(home, 'config', file);
    mkdirSync(dirname(to), { recursive: true });
    copyFileSync(from, to);
    copied++;
    console.log(`${id}: config/${file}`);
  }
}
console.log(`${copied} file(s) copied into ${join(home, 'config')}`);
