import { readFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
export const root = resolve(import.meta.dir, '../..');
export const engine = root;
export const manifest = join(engine, 'Cargo.toml');
export async function run(command: string[], cwd = root, env = process.env): Promise<void> {
  const child = Bun.spawn(command, { cwd, env, stdin: 'inherit', stdout: 'inherit', stderr: 'inherit' });
  const code = await child.exited;
  if (code !== 0) throw new Error(`${command[0]} exited with status ${code}`);
}
export function help(message: string): boolean {
  if (!process.argv.slice(2).some(arg => arg === '--help' || arg === '-h')) return false;
  console.log(message);
  return true;
}
/** The mods pack/pack.toml turns on: their Cargo features and content folders. */
export type PackMod = { id: string; name: string; feature?: string; content?: string };
export function packMods(): PackMod[] {
  const pack = Bun.TOML.parse(readFileSync(join(root, 'pack/pack.toml'), 'utf8')) as {
    mods: Record<string, { name: string; enabled: boolean; feature?: string; content?: string }>;
  };
  return Object.entries(pack.mods).filter(([, mod]) => mod.enabled).map(([id, mod]) => ({ id, name: mod.name, feature: mod.feature, content: mod.content }));
}
/** The Cargo features of the enabled mods, for the client build. */
export function packFeatures(): string {
  return [...new Set(packMods().flatMap(mod => mod.feature ? [mod.feature] : []))].join(',');
}
/** The content folder of an addon `pack.toml` lists with `content`: under
 * AE2_ADDON_CONTENT_ROOT, or the repository's content/. */
export function addonContent(id: string): string {
  const mod = packMods().find(mod => mod.id === id);
  if (!mod?.content) throw new Error(`pack/pack.toml has no enabled mod ${id} with a content folder`);
  return resolve(process.env.AE2_ADDON_CONTENT_ROOT ?? join(root, 'content'), mod.content);
}
/** The content folders of every enabled addon. */
export function addonContents(): { id: string; path: string }[] {
  return packMods().filter(mod => mod.content).map(mod => ({ id: mod.id, path: addonContent(mod.id) }));
}
