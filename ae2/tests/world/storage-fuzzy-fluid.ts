/** A physical fuzzy View Cell matches fluid variants by their primary fluid. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('storage-fuzzy-fluid', mode);
const visible = (view: any, item: string) => (view.slots ?? []).filter((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === item).length;
try {
  const state = await fixture.start();
  const [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), bench = at(4);
  await fixture.command('clear @s');
  await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 4, 3)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 3)} minecraft:stone`);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`setblock ${terminal} ae2:cable_bus`);
  await fixture.command(`setblock ${bench} ae2:cell_workbench`);
  // A fixture-only prefilled physical cell distinguishes two water keys,
  // one with an opaque fluid-component payload.
  const contents = 'ae2:fluid_storage_cell_1k[minecraft:custom_data={ae2_fluid_cell:{version:1,entries:[{fluid:"minecraft:water",components:[B;],amount:1000L},{fluid:"minecraft:water",components:[B;1b,2b,3b],amount:1000L},{fluid:"minecraft:lava",components:[B;],amount:1000L}]}}]';
  await fixture.command(`item replace block ${drive} container.0 with ${contents}`);
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  await until(() => fixture.ae(`grid status ${drive}`), view => view.online && view.powered, 'powered fluid drive');
  let fluids = await until(() => fixture.ae(`fluid grid status ${drive}`), view => view.entries?.length === 3, 'three physical fluid variants');
  assert.equal(fluids.entries.filter((entry: any) => entry.fluid === 'minecraft:water').length, 2);
  assert.equal(fluids.entries.filter((entry: any) => entry.fluid === 'minecraft:water' && entry.has_components).length, 1);

  // Fluid storage cells do not accept fuzzy cards in AE2. A View Cell does,
  // and supports fluid ghost filters at the Workbench.
  await fixture.replace('hotbar.0', 'ae2:view_cell'); await fixture.raw('hotbar 2');
  await fixture.command(`tp @s ${x + 4.5} ${y} ${z - 2.5}`);
  await fixture.raw(`interact ${bench} north`);
  await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot === 300), 'open workbench');
  await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'ae2:view_cell', 'view cell carried');
  await fixture.raw('click-slot 300');
  await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot === 300 && slot.item === 'ae2:view_cell'), 'physical view cell installed');
  await fixture.raw('close');
  await fixture.replace('hotbar.1', 'ae2:fuzzy_card'); await fixture.select(1, 'ae2:fuzzy_card');
  assert.equal((await fixture.ae(`workbench install ${bench}`)).configured, true);
  await fixture.replace('hotbar.2', 'minecraft:water_bucket'); await fixture.select(2, 'minecraft:water_bucket');
  assert.equal((await fixture.ae(`workbench filter_add_fluid ${bench}`)).configured, true);
  assert.equal((await fixture.ae(`workbench fuzzy ${bench} percent_50`)).configured, true);
  const configured = await fixture.ae(`workbench status ${bench}`);
  assert(configured.upgrades.some((entry: any) => entry.item === 'ae2:fuzzy_card'));
  assert(configured.filters.some((entry: any) => entry.fluid === 'minecraft:water'));

  await fixture.raw('hotbar 1'); await fixture.raw(`interact ${bench} north`);
  await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot === 300 && slot.item === 'ae2:view_cell'), 'configured cell in workbench');
  await fixture.raw('click-slot 300');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'ae2:view_cell', 'configured view cell carried');
  await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), view => !view.carried && view.hotbar?.[0]?.item === 'ae2:view_cell', 'configured view cell in hotbar');
  await fixture.raw('close');

  await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  await until(() => fixture.raw('screen'), view => visible(view, 'minecraft:water_bucket') === 2 && visible(view, 'minecraft:lava_bucket') === 1, 'unfiltered fluid variants in terminal');
  await fixture.raw('click-slot 0 shift');
  await until(() => fixture.raw('screen'), view => visible(view, 'minecraft:water_bucket') === 2 && visible(view, 'minecraft:lava_bucket') === 0, 'fuzzy view includes both water keys and excludes lava');
  fixture.mark('the physical fuzzy View Cell shows both water component variants and excludes lava');
  await fixture.raw('close'); await fixture.restart();
  await until(() => fixture.ae(`grid status ${drive}`), view => view.online && view.powered, 'reloaded fluid drive');
  fluids = await fixture.ae(`fluid grid status ${drive}`); assert.equal(fluids.entries.length, 3);
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  await until(() => fixture.raw('screen'), view => visible(view, 'minecraft:water_bucket') === 2 && visible(view, 'minecraft:lava_bucket') === 0, 'view cell survives restart');
  await fixture.raw('click-slot 360 shift');
  await until(() => fixture.raw('screen'), view => visible(view, 'minecraft:water_bucket') === 2 && visible(view, 'minecraft:lava_bucket') === 1, 'removing the view cell restores unfiltered fluids');
  await until(() => fixture.raw('inventory'), view => view.counts?.['ae2:view_cell'] === 1, 'physical view cell returned');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the view filter and three stored fluid variants persist without losing the cell');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
