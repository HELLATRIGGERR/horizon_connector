/** Physical menus honor an item's component limit and retain exact custody. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('physical-menu-stack-limit', mode);
const limited = 'minecraft:diamond[minecraft:max_stack_size=5]';
const cell = 'ae2:item_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:diamond",components:{"minecraft:max_stack_size":5},"#":10L}]]';
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
const diamonds = (view: any) => [...(view.hotbar ?? []), ...(view.main ?? [])].filter((item: any) => item?.item === 'minecraft:diamond');
const stored = (view: any) => view.items?.find((item: any) => item.item === 'minecraft:diamond')?.count ?? 0;

try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number) => `${x + dx} ${y} ${z}`;
  const chest = at(0), drive = at(3), terminal = at(4);
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  for (const [dx, dz] of [[-2, -3], [-2, 3], [6, -3], [6, 3]] as const) {
    await until(() => fixture.raw(`block ${x + dx} ${y} ${z + dz}`), state => state.loaded, 'fixture corner chunk loaded', 30000);
  }
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 6} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 6} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${chest} minecraft:chest[facing=north]`);
  await fixture.command(`item replace block ${chest} container.0 with ${limited} 5`);
  await fixture.command('clear @s');
  await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0', limited, 3);
  await fixture.replace('hotbar.8', 'minecraft:stick');
  assert.match(JSON.stringify(await fixture.command(`data get block ${chest}`)), /minecraft:max_stack_size/, 'physical chest stack has the source component on the server');
  await fixture.select(0, 'minecraft:diamond');
  assert.match(JSON.stringify(await fixture.command('data get entity @s SelectedItem')), /minecraft:max_stack_size/, 'player stack has the source component on the server');
  await until(() => fixture.raw('inventory'), view => String(view.hotbar[0]?.components ?? '').includes('minecraft:max_stack_size'), 'source component reaches the client');
  await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + .5} ${y} ${z - 2.5}`);
  await fixture.raw(`interact ${chest} north`);
  await until(() => fixture.raw('screen'), view => slot(view, 300)?.item === 'minecraft:diamond' && slot(view, 300)?.count === 5, 'physical chest with limited stack');
  await fixture.raw('click-slot 300 shift');
  // A server command round trip keeps a predicted client view from satisfying the test.
  await fixture.command(`data get block ${chest}`);
  const moved = await until(() => fixture.raw('inventory'), view => diamonds(view).reduce((n: number, item: any) => n + item.count, 0) === 8 && diamonds(view).length === 2, 'shift transfer splits at the component limit');
  assert.equal(moved.hotbar[0].count, 5);
  assert.deepEqual(diamonds(moved).map((item: any) => item.count).sort((a: number, b: number) => a - b), [3, 5]);
  assert(diamonds(moved).every((item: any) => String(item.components).includes('minecraft:max_stack_size')));
  const remainder = diamonds(moved).find((item: any) => item.count === 3);
  await fixture.raw(`click-slot ${remainder.slot}`);
  await fixture.raw('click-slot 0');
  await fixture.command(`data get block ${chest}`);
  await until(() => fixture.raw('inventory'), view => view.hotbar[0]?.count === 5 && view.carried?.item === 'minecraft:diamond' && view.carried.count === 3, 'a full stack refuses further cursor merging');
  await fixture.raw('click-slot 1');
  await until(() => fixture.raw('inventory'), view => !view.carried && view.hotbar[1]?.count === 3, 'remainder placed in an empty player slot');
  await fixture.raw('click-slot 1 shift');
  await fixture.command(`data get block ${chest}`);
  await until(() => fixture.raw('screen'), view => slot(view, 300)?.count === 3 && !slot(view, 1)?.item, 'player stack shift-moves back into the physical chest');
  await fixture.raw('close');
  fixture.mark('physical chest quick-move and cursor merge respect max five and conserve all eight diamonds');

  await fixture.command(`setblock ${at(2)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`setblock ${terminal} ae2:cable_bus`);
  await fixture.command(`item replace block ${drive} container.0 with ${cell}`);
  await fixture.command('clear @s');
  await fixture.replace('hotbar.8', 'ae2:terminal');
  await fixture.select(8, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const grid = () => until(() => fixture.ae(`grid status ${drive}`), state => state.online && state.powered, 'powered terminal grid', 30000);
  assert.equal(stored(await grid()), 10);
  await fixture.replace('hotbar.8', 'minecraft:stick');
  await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 4.5} ${y} ${z - 2.5}`);
  await fixture.raw(`interact ${terminal} north`);
  const view = await until(() => fixture.raw('screen'), state => state.slots?.some((item: any) => item.slot >= 300 && item.slot < 345 && item.item === 'minecraft:diamond'), 'terminal shows the source-keyed item');
  const entry = view.slots.find((item: any) => item.slot >= 300 && item.slot < 345 && item.item === 'minecraft:diamond');
  await fixture.raw(`click-slot ${entry.slot}`);
  await fixture.ae(`grid status ${drive}`);
  const taken = await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:diamond' && state.carried.count === 5, 'terminal extracts only one component-limited physical stack');
  assert.match(String(taken.carried.components), /minecraft:max_stack_size/);
  assert.equal(stored(await grid()), 5);
  await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), state => !state.carried && state.hotbar[0]?.count === 5, 'terminal cursor stack enters player inventory');
  await fixture.raw('click-slot 0 shift');
  await until(grid, state => stored(state) === 10, 'physical shift transfer returns five to the network');
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:diamond'] ?? 0, 0);
  await fixture.raw(`click-slot ${entry.slot}`);
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:diamond' && state.carried.count === 5, 'second extraction on cursor');
  await fixture.raw('close');
  await until(() => fixture.raw('inventory'), state => !state.carried && state.counts?.['minecraft:diamond'] === 5, 'close returns the physical cursor exactly once');
  await fixture.restart();
  const saved = await fixture.raw('inventory');
  assert.equal(saved.counts?.['minecraft:diamond'], 5);
  assert(diamonds(saved).every((item: any) => item.count <= 5 && String(item.components).includes('minecraft:max_stack_size')));
  assert.equal(stored(await grid()), 5);
  assert.match(JSON.stringify(await fixture.command(`data get block ${chest}`)), /minecraft:max_stack_size/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('terminal extraction, shift return and close preserve the source limit and exact inventory/network custody across restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
