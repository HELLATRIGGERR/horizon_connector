/** Physical cake recipes: source-condensed input nodes and returned milk buckets. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('crafting-remainders', mode);
const count = (status: any, item: string) => status.items?.find((entry: any) => entry.item === item)?.count ?? 0;
try {
  const state = await fixture.start();
  const [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), cpu = at(2, 1), provider = at(3), assembler = at(3, 0, -1);
  await fixture.command('clear @s');
  await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 5, 3)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 3)} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'], [cpu, '1k_crafting_storage'], [provider, 'pattern_provider'], [assembler, 'molecular_assembler']] as const) await fixture.command(`setblock ${pos} ae2:${id}`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const ready = () => until(() => fixture.ae(`grid status ${drive}`), s => s.online && s.powered, 'powered drive', 30000);
  await ready();
  await until(() => fixture.ae(`grid status ${assembler}`), s => s.online && s.powered, 'assembler online', 30000);

  await fixture.replace('hotbar.0', 'ae2:blank_pattern'); await fixture.select(0, 'ae2:blank_pattern');
  const encoded = await fixture.ae('crafting encode minecraft:milk_bucket minecraft:milk_bucket minecraft:milk_bucket minecraft:sugar minecraft:egg minecraft:sugar minecraft:wheat minecraft:wheat minecraft:wheat');
  assert.deepEqual([encoded.encoded, encoded.output, encoded.count], [true, 'minecraft:cake', 1]);
  await until(() => fixture.raw('inventory'), view => view.hotbar[0]?.item === 'ae2:crafting_pattern', 'physical cake pattern');
  await fixture.command(`tp @s ${x + 3.5} ${y} ${z + 2.5}`);
  await fixture.raw(`interact ${provider} south`);
  const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern'), 'physical provider menu', 5000);
  await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern').slot} shift`);
  await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:crafting_pattern'], 'cake pattern installed');
  await fixture.raw('close');
  async function deposit(item: string, amount: number) {
    await fixture.replace('hotbar.0', item, amount); await fixture.select(0, item);
    assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, amount);
  }
  for (let i = 0; i < 6; i++) await deposit('minecraft:milk_bucket', 1);
  await deposit('minecraft:sugar', 4); await deposit('minecraft:egg', 2); await deposit('minecraft:wheat', 6);
  const before = await ready();
  for (const [item, amount] of [['minecraft:milk_bucket', 6], ['minecraft:sugar', 4], ['minecraft:egg', 2], ['minecraft:wheat', 6]] as const) assert.equal(count(before, item), amount);
  fixture.mark('one physical cake pattern and the exact ingredients for two cakes are installed');

  await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  const view = await until(() => fixture.raw('screen'), screen => screen.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:cake'), 'craftable cake');
  await fixture.raw(`click-slot ${view.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:cake').slot}`);
  await until(() => fixture.raw('screen'), screen => JSON.stringify(screen.title ?? '').includes('AE2 Craft Amount'), 'craft amount');
  await fixture.raw('ui-key right'); await fixture.raw('ui-key backspace'); await fixture.raw('type 2'); await fixture.raw('wait 5'); await fixture.raw('click-slot 302');
  const confirm = await until(() => fixture.raw('screen'), screen => JSON.stringify(screen.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(screen)), 'cake confirmation');
  // 5 source tree nodes * 8 + 2 requested cakes + 18 inputs + 6 returned buckets + 2 pushes.
  assert.match(JSON.stringify(confirm), /68 bytes/);
  assert.doesNotMatch(JSON.stringify(confirm), /Missing materials/);
  const preview = await ready();
  for (const [item, amount] of [['minecraft:milk_bucket', 6], ['minecraft:sugar', 4], ['minecraft:egg', 2], ['minecraft:wheat', 6]] as const) assert.equal(count(preview, item), amount);
  await fixture.raw('click-slot 302'); await fixture.raw('close');
  const done = await until(() => fixture.ae(`grid status ${drive}`), s => count(s, 'minecraft:cake') === 2 && count(s, 'minecraft:bucket') === 6, 'two cakes and six physical returned buckets', 90000);
  for (const item of ['minecraft:milk_bucket', 'minecraft:sugar', 'minecraft:egg', 'minecraft:wheat']) assert.equal(count(done, item), 0);
  fixture.mark('68-byte source calculation and exactly two cakes plus six physical bucket remainders');
  await fixture.restart(); const saved = await ready();
  assert.equal(count(saved, 'minecraft:cake'), 2); assert.equal(count(saved, 'minecraft:bucket'), 6);
  for (const item of ['minecraft:milk_bucket', 'minecraft:sugar', 'minecraft:egg', 'minecraft:wheat']) assert.equal(count(saved, item), 0);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('finished output and all returned buckets survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
