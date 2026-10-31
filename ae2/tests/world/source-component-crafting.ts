/** Source-shaped cells and crafting patterns retain physical contents through a job and restart. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('source-component-crafting', mode);
const count = (status: any, item: string) => status.items?.find((entry: any) => entry.item === item)?.count ?? 0;
const cell = 'ae2:item_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:oak_log","#":2L},{"#t":"ae2:i",id:"removed:ore","#":3L}]]';
const fluidCell = 'ae2:fluid_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":1000L}]]';
const pattern = 'ae2:crafting_pattern[ae2:encoded_crafting_pattern={inputs:[{id:"minecraft:oak_log",count:1},{},{},{},{},{},{},{},{}],result:{id:"minecraft:oak_planks",count:4},recipeId:"minecraft:oak_planks",canSubstitute:0b,canSubstituteFluids:0b}]';
try {
  const state = await fixture.start();
  const [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), cpu = at(2, 1), provider = at(3), assembler = at(3, 0, -1);
  await fixture.command('clear @s');
  await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 5, 3)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 3)} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'],
    [cpu, '64k_crafting_storage'], [provider, 'pattern_provider'], [assembler, 'molecular_assembler']] as const) {
    await fixture.command(`setblock ${pos} ae2:${id}`);
  }
  await fixture.command(`item replace block ${drive} container.0 with ${cell}`);
  await fixture.command(`item replace block ${drive} container.1 with ${fluidCell}`);
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const ready = () => until(() => fixture.ae(`grid status ${drive}`), value => value.online && value.powered, 'powered source-cell drive', 30000);
  await ready();
  for (const pos of [provider, assembler]) await until(() => fixture.ae(`grid status ${pos}`), value => value.online && value.powered, `online ${pos}`, 30000);
  await until(ready, value => count(value, 'minecraft:oak_log') === 2 && count(value, 'ae2:missing_content') === 3, 'source item cell contents and missing-content recovery');
  const fluids = await until(() => fixture.ae(`fluid grid status ${drive}`), value => value.entries?.some((entry: any) => entry.fluid === 'minecraft:water'), 'source fluid cell contents');
  assert(fluids.entries.some((entry: any) => entry.fluid === 'minecraft:water' && entry.amount_mb === 1000));
  fixture.mark('source item and fluid cells expose physical amounts and convert an unknown key into recoverable Missing Content');

  await fixture.replace('hotbar.0', pattern); await fixture.replace('hotbar.8', 'minecraft:blaze_rod');
  await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 5.5} ${y} ${z + 0.5}`); await fixture.raw(`interact ${provider} east`);
  const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern'), 'source pattern in player inventory');
  await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern').slot} shift`);
  await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:crafting_pattern'], 'source pattern physically installed');
  await fixture.raw('close');
  const providerData = (await fixture.command(`data get block ${provider}`)).chat?.join('\n') ?? '';
  assert.match(providerData, /ae2:encoded_crafting_pattern/);
  assert.doesNotMatch(providerData, /ae2_lab_pattern/);
  await fixture.restart(); await ready();
  await until(ready, value => count(value, 'minecraft:oak_log') === 2 && count(value, 'ae2:missing_content') === 3, 'source cell and missing-content key survive reload');
  fixture.mark('the source-only pattern and source cell survive reload before planning');

  await fixture.select(8, 'minecraft:blaze_rod'); await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  const screen = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks'), 'source pattern appears as craftable');
  await fixture.raw(`click-slot ${screen.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks').slot} shift`);
  await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), 'source-pattern amount');
  await fixture.raw('ui-key right'); await fixture.raw('ui-key backspace'); await fixture.raw('type 8'); await fixture.raw('click-slot 302');
  const confirm = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), 'source-pattern confirmation');
  assert.doesNotMatch(JSON.stringify(confirm), /Missing materials/);
  assert.match(JSON.stringify(confirm), /Planned 8 x minecraft:oak_planks/);
  await fixture.raw('click-slot 302'); await fixture.raw('close');
  await until(ready, value => count(value, 'minecraft:oak_log') === 0 && count(value, 'minecraft:oak_planks') === 8, 'source recipe completes with two physical logs', 90000);
  await fixture.restart();
  const saved = await ready();
  assert.equal(count(saved, 'minecraft:oak_log'), 0); assert.equal(count(saved, 'minecraft:oak_planks'), 8);
  assert.equal(count(saved, 'ae2:missing_content'), 3);
  assert.match((await fixture.command(`data get block ${drive}`)).chat?.join('\n') ?? '', /removed:ore/, 'the original unavailable item key remains recoverable');
  assert((await fixture.ae(`fluid grid status ${drive}`)).entries.some((entry: any) => entry.fluid === 'minecraft:water' && entry.amount_mb === 1000));
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the exact source recipe consumes both logs, produces eight planks and preserves the fluid through restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
