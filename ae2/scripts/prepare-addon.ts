/** Generate an AE2 addon's content mod from the player's All the Mods 10 jar.
 * What the addon's slices declare is in ae2/addons/<id>/content.json, where
 * the jar comes from in its SOURCE_MANIFEST.json. Generated files stay local. */
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { root, help } from './common';

type Declared = { fire_resistant?: boolean; max_stack_size?: number };
type Block = Declared & { template?: string; properties?: Record<string, unknown> };
type Content = { namespace: string; name: string; description: string; authors: string[]; slices: string[];
  items: Record<string, Declared>; blocks: Record<string, Block> };
type Manifest = { addon: string; name: string; version: string; source: { url: string; tag: string; commit: string; license: string };
  assets_license: string; jar: { file: string; sha256: string } };

const usage = 'Usage: ATM10_DIR="<ATM10 instance>" bun run ae2/scripts/prepare-addon.ts <addon id>\n'
  + 'Writes content/<namespace>/ (or <AE2_ADDON_CONTENT_ROOT>/<namespace>/, or AE2_ADDON_CONTENT_DIR): the addon\'s assets and data from its jar in the instance\'s mods/,\n'
  + 'with the blocks and items ae2/addons/<id>/content.json declares and the recipes, loot tables and tags of those.\n'
  + 'A folder this script wrote before is replaced; any other non-empty destination is refused.';
if (!help(usage)) {
  const id = process.argv[2];
  if (!id || !/^[a-z0-9_]+$/.test(id)) throw new Error(usage);
  const addon = join(root, 'ae2/addons', id);
  const manifest: Manifest = JSON.parse(readFileSync(join(addon, 'SOURCE_MANIFEST.json'), 'utf8'));
  const content: Content = JSON.parse(readFileSync(join(addon, 'content.json'), 'utf8'));
  const ns = content.namespace;
  const atm10 = process.env.ATM10_DIR;
  if (!atm10 || !existsSync(join(atm10, 'mods'))) throw new Error('Set ATM10_DIR to an All the Mods 10 instance (the folder that holds its mods/).');
  const jar = join(atm10, 'mods', manifest.jar.file);
  if (!existsSync(jar)) throw new Error(`${manifest.jar.file} is not in ${join(atm10, 'mods')}; this port follows ${manifest.name} ${manifest.version}.`);
  const sha = createHash('sha256').update(readFileSync(jar)).digest('hex');
  if (sha !== manifest.jar.sha256) throw new Error(`${jar}: SHA-256 ${sha} is not the pinned ${manifest.jar.sha256}`);

  const out = resolve(process.env.AE2_ADDON_CONTENT_DIR ?? join(process.env.AE2_ADDON_CONTENT_ROOT ?? join(root, 'content'), ns));
  const marker = join(out, 'content-inventory.json');
  if (existsSync(out) && readdirSync(out).length) {
    const generated = existsSync(marker) && JSON.parse(readFileSync(marker, 'utf8')).generator === 'ae2/scripts/prepare-addon.ts';
    if (!generated) throw new Error(`Content destination is not empty and was not written by this script: ${out}`);
    rmSync(out, { recursive: true, force: true });
  }
  // A fresh copy of the jar's files, outside the content.
  const jarDir = join(root, '.local/addons', id, 'jar');
  rmSync(jarDir, { recursive: true, force: true }); mkdirSync(jarDir, { recursive: true });
  const unzip = spawnSync('unzip', ['-q', '-o', jar, '-d', jarDir], { stdio: 'inherit' });
  if (unzip.error) throw unzip.error;
  if (unzip.status !== 0) throw new Error(`unzip failed (${unzip.status})`);

  const walk = (dir: string): string[] => existsSync(dir) ? readdirSync(dir).flatMap(name => {
    const path = join(dir, name); return statSync(path).isDirectory() ? walk(path) : [path];
  }) : [];
  const read = (path: string) => JSON.parse(readFileSync(path, 'utf8'));
  const put = (path: string, value: unknown) => { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, JSON.stringify(value, null, 2) + '\n'); };
  const copy = (from: string, to: string) => { mkdirSync(dirname(to), { recursive: true }); cpSync(from, to); };
  const assets = join(jarDir, 'assets', ns), data = join(jarDir, 'data');
  const blocks = new Set(Object.keys(content.blocks).map(b => `${ns}:${b}`));
  // Block items are items too.
  const items = new Set([...Object.keys(content.items).map(i => `${ns}:${i}`), ...blocks]);
  const report: any = { generator: 'ae2/scripts/prepare-addon.ts', addon: id, source: manifest.source, jar: manifest.jar,
    slices: content.slices, items: [...items].sort(), blocks: [...blocks].sort(),
    recipes: [] as string[], recipes_left_out: {} as Record<string, string>, loot_tables: [] as string[], tags: [] as string[], deferred: {} as Record<string, number> };

  // Assets: language files, textures and models whole (a model no blockstate
  // or item names is never read), blockstates and item definitions for the
  // declared blocks and items.
  for (const type of ['lang', 'textures', 'models']) {
    if (existsSync(join(assets, type))) cpSync(join(assets, type), join(out, 'assets', ns, type), { recursive: true });
  }
  const fireResistant = { 'minecraft:damage_resistant': { types: '#minecraft:is_fire' } };
  for (const [block, spec] of Object.entries(content.blocks)) {
    const state = join(assets, 'blockstates', block + '.json');
    if (!existsSync(state)) throw new Error(`${ns}:${block} has no blockstate in the jar`);
    copy(state, join(out, 'assets', ns, 'blockstates', block + '.json'));
    put(join(out, 'data', ns, 'horizon/block', block + '.json'), {
      template: spec.template ?? 'minecraft:iron_block', item: true,
      ...(spec.properties ? { properties: spec.properties } : {}),
      ...(spec.fire_resistant ? { components: fireResistant } : {}),
    });
  }
  for (const [item, spec] of Object.entries(content.items)) {
    put(join(out, 'data', ns, 'horizon/item', item + '.json'), {
      template: 'minecraft:iron_ingot',
      ...(spec.max_stack_size ? { max_stack_size: spec.max_stack_size } : {}),
      ...(spec.fire_resistant ? { components: fireResistant } : {}),
    });
  }
  for (const item of items) {
    const path = item.slice(ns.length + 1);
    if (!existsSync(join(assets, 'models/item', path + '.json'))) throw new Error(`${item} has no item model in the jar`);
    put(join(out, 'assets', ns, 'items', path + '.json'), { model: { type: 'minecraft:model', model: `${ns}:item/${path}` } });
  }

  // Recipes: those of the declared items only, in their 1.21.1 form, which
  // the port reads (AE2's own are kept so too). A recipe naming an item of
  // the addon not declared yet, or with NeoForge conditions, waits for its slice.
  const ids = (value: unknown, found: Set<string>): Set<string> => {
    if (typeof value === 'string') { if (value.startsWith(ns + ':')) found.add(value); }
    else if (Array.isArray(value)) value.forEach(v => ids(v, found));
    else if (value && typeof value === 'object') for (const [key, v] of Object.entries(value)) if (key !== 'type') ids(v, found);
    return found;
  };
  const recipes = join(data, ns, 'recipe');
  for (const file of walk(recipes).filter(p => p.endsWith('.json'))) {
    const name = `${ns}:${relative(recipes, file).replaceAll('\\', '/').slice(0, -5)}`;
    const recipe = read(file);
    const missing = [...ids(recipe, new Set())].filter(i => !items.has(i));
    if (recipe['neoforge:conditions']) report.recipes_left_out[name] = `conditions ${JSON.stringify(recipe['neoforge:conditions'])}`;
    else if (missing.length) report.recipes_left_out[name] = `needs ${missing.join(', ')}`;
    else { copy(file, join(out, 'data', ns, 'recipe', relative(recipes, file))); report.recipes.push(name); }
  }
  for (const block of Object.keys(content.blocks)) {
    const table = join(data, ns, 'loot_table/blocks', block + '.json');
    if (existsSync(table)) { copy(table, join(out, 'data', ns, 'loot_table/blocks', block + '.json')); report.loot_tables.push(`${ns}:blocks/${block}`); }
  }
  // Tags the addon adds to: its own, vanilla's and the convention (c) ones,
  // with only the declared entries. A reference to one of these tags that
  // ends up empty goes too; other tag references stay.
  const tagFiles = ['c', 'minecraft', ns].flatMap(space => walk(join(data, space, 'tags')).filter(p => p.endsWith('.json')).map(p => ({ space, path: p })));
  const tagId = (space: string, path: string) => {
    const rel = relative(join(data, space, 'tags'), path).replaceAll('\\', '/').slice(0, -5);
    const [kind, ...rest] = rel.split('/'); return { kind, id: `${space}:${rest.join('/')}` };
  };
  const kept = new Map<string, unknown[]>();
  for (let changed = true; changed;) {
    changed = false;
    for (const { space, path } of tagFiles) {
      const { kind, id: tag } = tagId(space, path);
      const known = kind === 'block' ? blocks : items;
      const own = new Set(tagFiles.map(f => tagId(f.space, f.path)).filter(t => t.kind === kind).map(t => t.id));
      const values = (read(path).values as unknown[]).filter(v => {
        const ref = typeof v === 'string' ? v : (v as any).id;
        if (ref.startsWith('#')) return !own.has(ref.slice(1)) || (kept.get(`${kind}/${ref.slice(1)}`)?.length ?? 0) > 0;
        return !ref.startsWith(ns + ':') || known.has(ref);
      });
      const key = `${kind}/${tag}`;
      if (JSON.stringify(kept.get(key)) !== JSON.stringify(values)) { kept.set(key, values); changed = true; }
    }
  }
  for (const { space, path } of tagFiles) {
    const { kind, id: tag } = tagId(space, path);
    const values = kept.get(`${kind}/${tag}`) ?? [];
    if (!values.length) continue;
    put(join(out, 'data', space, 'tags', relative(join(data, space, 'tags'), path)), { ...read(path), values });
    report.tags.push(`${kind}/${tag}`);
  }
  // Files left for later slices: AE2's namespace (screens, GUI textures,
  // tags), which belong in content/ae2, the guide, data maps, advancements.
  const later = (label: string, dir: string) => { const n = walk(dir).length; if (n) report.deferred[label] = n; };
  later('assets/ae2 (screens and GUI textures, merged into content/ae2 with the slices that use them)', join(jarDir, 'assets/ae2'));
  later('data/ae2 (tags, merged into content/ae2 with the slices that use them)', join(data, 'ae2'));
  later(`assets/${ns}/ae2guide`, join(assets, 'ae2guide'));
  later(`data/${ns}/data_maps`, join(data, ns, 'data_maps'));
  later(`data/${ns}/advancement`, join(data, ns, 'advancement'));

  writeFileSync(join(out, 'horizon-mod.toml'), [
    `id = "${ns}"`, `name = "${content.name}"`, `version = "${manifest.version}"`,
    `description = "${content.description}"`, `authors = ${JSON.stringify(content.authors)}`,
    'api = "0.2"', 'capabilities = ["registries"]', ''].join('\n'));
  writeFileSync(join(out, 'ATTRIBUTION.md'), `# Attribution\n\nGenerated locally from ${manifest.name} ${manifest.version} (${manifest.jar.file}, SHA-256 ${manifest.jar.sha256}), `
    + `source ${manifest.source.url} tag ${manifest.source.tag}, commit ${manifest.source.commit}.\n\n`
    + `${manifest.name} code: ${manifest.source.license}. Its assets: ${manifest.assets_license}, https://creativecommons.org/licenses/by-nc-sa/3.0/ ; `
    + 'they cannot be used commercially. This folder is not part of the repository and is not redistributed.\n');
  report.counts = { items: items.size, blocks: blocks.size, recipes: report.recipes.length, recipes_left_out: Object.keys(report.recipes_left_out).length,
    loot_tables: report.loot_tables.length, tags: report.tags.length };
  put(marker, report);
  console.log(JSON.stringify(report.counts, null, 2));
  console.log(`${manifest.name} content prepared in ${out}. It stays local, as content/ae2 does.`);
}
