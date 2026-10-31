/** Shared AEBaseScreen fake drags keep machine cells, cards and cursor physical. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('fake-slot-drag-machines', 'server-thread');
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
try {
  const start = await fixture.start(); const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const at = (dx: number) => `${x + dx} ${y} ${z}`, iface = at(0), bench = at(3), bus = at(6);
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 8} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 8} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${iface} ae2:interface`); await fixture.command(`setblock ${bench} ae2:cell_workbench`); await fixture.command(`setblock ${bus} ae2:cable_bus`);
  await fixture.command('clear @s'); await fixture.replace('hotbar.0', 'ae2:import_bus'); await fixture.select(0, 'ae2:import_bus');
  assert.equal((await fixture.ae(`grid part ${bus} south`)).attached, true);
  await fixture.command('gamemode survival @s'); await fixture.replace('hotbar.0', 'minecraft:diamond', 4);
  await fixture.replace('hotbar.1', 'ae2:item_storage_cell_1k'); await fixture.replace('hotbar.2', 'ae2:speed_card', 2);
  await fixture.replace('hotbar.8', 'minecraft:stick');
  const open = async (target: string, dx: number, title: RegExp) => {
    await fixture.select(8, 'minecraft:stick'); await fixture.command(`tp @s ${x + dx + .5} ${y} ${z + 2.5}`); await fixture.raw(`interact ${target} south`);
    await until(() => fixture.raw('screen'), view => title.test(JSON.stringify(view.title ?? '')) && slot(view, 300), 'physical owner menu');
  };
  const drag = async (a: number, b: number, right = false) => {
    const widgets = await fixture.raw('ui');
    const center = (index: number): [number, number] => {
      const node = widgets.widgets.find((entry: any) => entry.id === `slot-${index}`); assert(node, `slot ${index} has a hit target`);
      return [node.bounds[0] + 8, node.bounds[1] + 8];
    };
    const [ax, ay] = center(a), [bx, by] = center(b);
    await fixture.raw(`drag-at ${right ? 'right ' : ''}${ax} ${ay} ${bx} ${by} ${ax} ${ay}`);
  };
  const holdDiamond = async () => {
    await fixture.raw('click-slot 0');
    await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:diamond' && view.carried.count === 4, 'real diamond cursor');
  };
  const returnDiamond = async () => {
    await fixture.raw('click-slot 0');
    await until(() => fixture.raw('inventory'), view => !view.carried && view.counts?.['minecraft:diamond'] === 4, 'real diamonds returned');
  };
  await open(iface, 0, /Interface|AE2 Machine/i); await holdDiamond();
  await drag(309, 310); await fixture.command(`data get block ${iface} ae2_lab_config`);
  let view = await until(() => fixture.raw('screen'), v => slot(v, 309)?.item === 'minecraft:diamond' && slot(v, 309)?.count === 4 && slot(v, 310)?.count === 4, 'Interface left drag copies full stack to each stock ghost');
  assert.equal(view.carried?.count, 4);
  await drag(309, 310, true); await fixture.command(`data get block ${iface} ae2_lab_config`);
  view = await until(() => fixture.raw('screen'), v => slot(v, 309)?.count === 5 && slot(v, 310)?.count === 5, 'Interface right drag adds one per ghost');
  assert.equal(view.carried?.count, 4); await returnDiamond(); await fixture.raw('close');

  await open(bench, 3, /AE2 Machine/i); await fixture.raw('click-slot 1'); await fixture.raw('click-slot 300');
  await fixture.command(`data get block ${bench}`);
  await until(() => fixture.raw('screen'), v => slot(v, 300)?.item === 'ae2:item_storage_cell_1k' && !v.carried, 'physical cell installed');
  await holdDiamond(); await drag(305, 306); await fixture.command(`data get block ${bench} ae2_lab_config`);
  view = await until(() => fixture.raw('screen'), v => slot(v, 305)?.item === 'minecraft:diamond' && slot(v, 306)?.item === 'minecraft:diamond' && v.carried?.count === 4, 'Workbench drag fills two type filters');
  assert.equal(slot(view, 300)?.item, 'ae2:item_storage_cell_1k'); await returnDiamond(); await fixture.raw('close');

  await open(bus, 6, /AE2 Bus/i); await holdDiamond(); await drag(300, 301); await fixture.command(`data get block ${bus} ae2_lab_config.parts`);
  view = await until(() => fixture.raw('screen'), v => slot(v, 300)?.item === 'minecraft:diamond' && slot(v, 301)?.item === 'minecraft:diamond' && v.carried?.count === 4, 'bus drag fills two type filters');
  await returnDiamond(); await fixture.raw('click-slot 2'); await until(() => fixture.raw('inventory'), v => v.carried?.item === 'ae2:speed_card' && v.carried.count === 2, 'physical upgrade cards held');
  // Real upgrade slots use vanilla physical quick-craft, not fake-slot actions.
  const widgets = await fixture.raw('ui'); const centers = [336,337].map(index => { const n=widgets.widgets.find((entry: any) => entry.id === `slot-${index}`); assert(n); return [n.bounds[0]+8,n.bounds[1]+8]; });
  await fixture.raw(`drag-at ${centers[0][0]} ${centers[0][1]} ${centers[1][0]} ${centers[1][1]}`); await fixture.command(`data get block ${bus} ae2_lab_config.parts`);
  await until(() => fixture.raw('screen'), v => slot(v, 336)?.item === 'ae2:speed_card' && slot(v, 337)?.item === 'ae2:speed_card' && !v.carried, 'physical speed cards distributed without duplication');
  await fixture.raw('close'); await fixture.restart();
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:diamond'], 4);
  assert.match(JSON.stringify((await fixture.command(`data get block ${bus} ae2_lab_config.parts`)).chat), /ae2:speed_card/);
  assert.match(JSON.stringify((await fixture.command(`data get block ${bench}`)).chat), /ae2:item_storage_cell_1k/);
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('Interface, Workbench and bus fake drags preserve templates while real cell and upgrade slots retain custody'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
