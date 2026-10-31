/** Broader Annihilation Plane block-entity drops and real item custody. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('annihilation-machine-containers', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 6, z = Math.floor(pz) + 6;
  const at = (dx = 0, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const host = at(), power = at(-1), drive = at(1), target = at(0, 1);
  await fixture.command(`fill ${at(-3, -2, -3)} ${at(4, -2, 3)} minecraft:stone`);
  await fixture.command(`fill ${at(-3, -1, -3)} ${at(4, 3, 3)} minecraft:air`);
  await fixture.command(`tp @s ${x + 2.5} ${y + 1} ${z - 2.5}`);
  await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:annihilation_plane');
  await fixture.select(0, 'ae2:annihilation_plane');
  await fixture.hold();
  await fixture.command(`setblock ${power} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`setblock ${host} ae2:cable_bus`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  assert.equal((await fixture.ae(`grid part ${host} up`)).attached, true);
  await fixture.stepServer(100);
  await until(() => fixture.ae(`grid status ${drive}`), s => s.online && s.powered, 'plane grid online');

  const broken = async () => {
    await fixture.stepServer(300);
    assert((await fixture.command(`execute if block ${target} minecraft:air`)).chat?.includes('Test passed'), 'physical target was removed');
  };
  const take = async (item: string, count: number) => {
    const grid = await fixture.ae(`grid status ${drive}`);
    const index = grid.items.findIndex((entry: any) => entry.item === item);
    assert(index >= 0 && grid.items[index].count === count, `${count} ${item} in real storage`);
    assert.equal((await fixture.ae(`grid withdraw ${drive} ${index} ${count}`)).extracted, count);
  };
  const inventory = async () => JSON.stringify(await fixture.command('data get entity @s Inventory'));

  await fixture.command(`setblock ${target} minecraft:shulker_box`);
  await fixture.command(`data merge block ${target} {Items:[{Slot:0b,id:"minecraft:emerald",count:3,components:{"minecraft:custom_data":{shulker_probe:31}}}]}`);
  assert.match(JSON.stringify(await fixture.command(`data get block ${target}`)), /shulker_probe/);
  await broken();
  const shulkerGrid = await fixture.ae(`grid status ${drive}`);
  assert(!shulkerGrid.items.some((entry: any) => entry.item === 'minecraft:emerald'), 'shulker contents are not spilled separately');
  await take('minecraft:shulker_box', 1);
  assert.match(await inventory(), /shulker_probe.{0,12}31/);
  fixture.mark('the shulker item keeps its component-bearing contents without separately spilling them');

  await fixture.command(`setblock ${target} minecraft:brewing_stand`);
  await fixture.command(`data merge block ${target} {Items:[{Slot:3b,id:"minecraft:diamond",count:2,components:{"minecraft:custom_data":{brew_probe:32}}}]}`);
  assert.match(JSON.stringify(await fixture.command(`data get block ${target}`)), /brew_probe/);
  await broken();
  await take('minecraft:brewing_stand', 1);
  await take('minecraft:diamond', 2);
  assert.match(await inventory(), /brew_probe.{0,12}32/);
  fixture.mark('the brewing stand and its physical inventory enter storage with item components');

  await fixture.command(`setblock ${target} ae2:inscriber`);
  await fixture.ae(`grid status ${target}`);
  await fixture.command(`item replace block ${target} container.2 with minecraft:emerald[minecraft:custom_data={machine_probe:33}]`);
  assert.match(JSON.stringify(await fixture.command(`data get block ${target}`)), /machine_probe/);
  await broken();
  await take('ae2:inscriber', 1);
  await take('minecraft:emerald', 1);
  assert.match(await inventory(), /machine_probe.{0,12}33/);
  fixture.mark('the AE2 machine and its physical input are each recovered exactly once');

  await fixture.command(`setblock ${target} ae2:energy_cell`);
  await fixture.ae(`grid status ${target}`);
  await fixture.command(`data merge block ${target} {ae2_lab_energy:12345.0d}`);
  assert((await fixture.ae(`power ${target} east`)).energy > 0);
  await broken();
  await take('ae2:energy_cell', 1);
  assert.match(await inventory(), /ae2:stored_energy/, 'dismantled energy uses the direct source component');
  fixture.mark('an energy-cell block drop carries its remaining source stored-energy component');

  await fixture.hold(false); await fixture.restart();
  const restored = await inventory();
  for (const marker of ['shulker_probe', 'brew_probe', 'machine_probe', 'ae2:stored_energy']) {
    assert(restored.includes(marker), `${marker} survives restart`);
  }
  const player = await until(() => fixture.raw('inventory'), state =>
    state.hotbar?.some((item: any) => item?.item === 'ae2:energy_cell'), 'stored energy cell is in the physical hotbar', 30000);
  const slot = player.hotbar.findIndex((item: any) => item?.item === 'ae2:energy_cell');
  await fixture.select(slot, 'ae2:energy_cell');
  const selected = (await fixture.command('data get entity @s SelectedItem')).chat.join('\n');
  const amount = selected.match(/ae2:stored_energy["']?\s*:\s*(-?\d+(?:\.\d+)?(?:[eE][-+]?\d+)?)/);
  assert(amount && Number(amount[1]) > 0, 'the source component records positive stored energy');
  await fixture.command(`tp @s ${x + 3.5} ${y - 1} ${z - 2.5}`);
  await fixture.raw(`interact ${at(3, -2)} up`);
  const placed = at(3, -1);
  await until(() => fixture.command(`execute if block ${placed} ae2:energy_cell`), reply =>
    reply.chat?.includes('Test passed'), 'the recovered energy cell is placed in the world', 30000);
  const placedEnergy = (await fixture.ae(`power ${placed} east`)).energy;
  assert.equal(placedEnergy, Number(amount[1]), 'placing the recovered source-component item restores its stored energy');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('component-bearing drops survive restart, and the recovered energy cell restores its stored power on placement');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
