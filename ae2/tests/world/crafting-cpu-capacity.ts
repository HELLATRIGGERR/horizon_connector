/** A physical 1K CPU rejects an oversized job and accepts its exact byte boundary. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('crafting-cpu-capacity', mode);
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
  await ready(); await until(() => fixture.ae(`grid status ${assembler}`), s => s.online && s.powered, 'assembler online', 30000);
  await fixture.replace('hotbar.0', 'ae2:blank_pattern'); await fixture.select(0, 'ae2:blank_pattern');
  const encoded = await fixture.ae('crafting encode minecraft:oak_log - - - - - - - -');
  assert.deepEqual([encoded.encoded, encoded.output, encoded.count], [true, 'minecraft:oak_planks', 4]);
  await until(() => fixture.raw('inventory'), view => view.hotbar[0]?.item === 'ae2:crafting_pattern', 'physical plank pattern');
  await fixture.command(`tp @s ${x + 3.5} ${y} ${z + 2.5}`); await fixture.raw(`interact ${provider} south`);
  const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern'), 'provider menu', 5000);
  await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern').slot} shift`);
  await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:crafting_pattern'], 'pattern installed'); await fixture.raw('close');
  for (let i = 0; i < 3; i++) {
    await fixture.replace('hotbar.0', 'minecraft:oak_log', 64); await fixture.select(0, 'minecraft:oak_log');
    assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 64);
  }
  assert.equal(count(await ready(), 'minecraft:oak_log'), 192);
  await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  async function confirm(amount: number) {
    await fixture.raw(`interact ${terminal} north`);
    const view = await until(() => fixture.raw('screen'), s => s.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks'), 'craftable planks');
    await fixture.raw(`click-slot ${view.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks').slot}`);
    await until(() => fixture.raw('screen'), s => JSON.stringify(s.title ?? '').includes('AE2 Craft Amount'), 'craft amount');
    await fixture.raw('ui-key right'); await fixture.raw('ui-key backspace'); await fixture.raw(`type ${amount}`); await fixture.raw('wait 5'); await fixture.raw('click-slot 302');
    return until(() => fixture.raw('screen'), s => JSON.stringify(s.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(s)), 'craft confirmation');
  }
  // 2 nodes * 8 + 768 requested planks + 192 input logs + 192 pushes.
  const oversized = await confirm(768); assert.match(JSON.stringify(oversized), /1168 bytes/);
  assert.doesNotMatch(JSON.stringify(oversized), /Missing materials/);
  await fixture.raw('click-slot 302'); await fixture.step(10);
  assert.match(JSON.stringify((await fixture.raw('screen')).title ?? ''), /AE2 Craft Confirm/);
  assert.doesNotMatch(JSON.stringify(await fixture.command(`data get block ${cpu}`)), /ae2_recursive_job/);
  assert.equal(count(await ready(), 'minecraft:oak_log'), 192); await fixture.raw('close');
  fixture.mark('a 1168-byte request leaves the 1K CPU idle and all 192 logs untouched');

  // 2 nodes * 8 + 672 requested planks + 168 input logs + 168 pushes = 1024.
  const boundary = await confirm(672); assert.match(JSON.stringify(boundary), /1024 bytes/);
  assert.doesNotMatch(JSON.stringify(boundary), /Missing materials/);
  await fixture.raw('click-slot 302'); await fixture.raw('close');
  const done = await until(() => fixture.ae(`grid status ${drive}`), s => count(s, 'minecraft:oak_planks') === 672 && count(s, 'minecraft:oak_log') === 24, 'exact-boundary CPU job', 120000);
  assert.equal(count(done, 'minecraft:oak_planks'), 672); assert.equal(count(done, 'minecraft:oak_log'), 24);
  fixture.mark('the exact 1024-byte job runs on the 1K CPU and consumes exactly 168 logs');
  await fixture.restart(); const saved = await ready();
  assert.equal(count(saved, 'minecraft:oak_planks'), 672); assert.equal(count(saved, 'minecraft:oak_log'), 24);
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('the physical output and remaining logs survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
