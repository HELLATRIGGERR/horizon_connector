/** Physical item Formation Plane placement, component custody and block placement. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('formation-item-components', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 5, z = Math.floor(pz) + 6;
  const at = (dx = 0, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const host = at(), target = at(1), below = at(1, -1), above = at(1, 1);
  const block = async (pos = target) => String((await fixture.raw(`getblock ${pos}`)).block);
  const data = async () => JSON.stringify(await fixture.command(`data get block ${target}`));
  const inventory = async () => JSON.stringify(await fixture.command('data get entity @s Inventory'));
  const offer = async (spec: string, item: string) => {
    // Inventory packets need normal server iterations to reach the hidden
    // client. Restore the barrier before testing the physical transfer.
    await fixture.hold(false);
    try {
      await fixture.replace('hotbar.1', spec);
      await fixture.select(1, item);
    } finally { await fixture.hold(); }
    await ready();
  };
  const ready = async () => {
    await fixture.stepServer(100);
    await until(() => fixture.ae(`grid part-status ${host} east`), state => state.online && state.powered, 'powered formation part', 30000);
  };
  const restart = async () => { await fixture.hold(false); await fixture.restart(); await fixture.hold(); await ready(); };
  const place = async (item: string) => {
    assert.equal((await fixture.ae(`grid deposit ${host}`)).inserted, 1, `one ${item} transferred`);
    assert.doesNotMatch(await inventory(), new RegExp(item.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')), `the player no longer owns ${item}`);
  };

  await fixture.command(`fill ${at(-3, -1, -3)} ${at(5, -1, 3)} minecraft:stone`);
  await fixture.command(`fill ${at(-3, 0, -3)} ${at(5, 4, 3)} minecraft:air`);
  await fixture.command(`tp @s ${x + 1.5} ${y + 1} ${z - 2.5}`);
  await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:formation_plane');
  await fixture.select(0, 'ae2:formation_plane');
  await fixture.hold();
  await fixture.command(`setblock ${at(-1)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${host} ae2:cable_bus`);
  assert.equal((await fixture.ae(`grid part ${host} east`)).attached, true);
  await ready();

  // A rejected placement must leave the real item and its components with the
  // player. The subsequent neighbor update must clear the source blocked bit.
  await fixture.command(`setblock ${target} minecraft:stone`);
  await offer('ae2:energy_cell[ae2:stored_energy=1234.0d,minecraft:custom_name={text:"Formation Battery",color:"red"}]', 'ae2:energy_cell');
  assert.equal((await fixture.ae(`grid deposit ${host}`)).inserted, 0);
  assert.match(await inventory(), /Formation Battery/);
  assert.match(await inventory(), /ae2:stored_energy/);
  assert.match(await block(), /^minecraft:stone(?:\[|$)/);
  await fixture.command(`setblock ${target} minecraft:air`);
  await place('ae2:energy_cell');
  assert.match(await block(), /^ae2:energy_cell(?:\[|$)/);
  assert.equal((await fixture.ae(`power ${target} east`)).energy, 1234);
  assert.match(await data(), /Formation Battery/);
  assert.match(await data(), /color.{0,12}red/);
  await restart();
  assert.equal((await fixture.ae(`power ${target} east`)).energy, 1234);
  assert.match(await data(), /Formation Battery/);
  assert.match(await data(), /color.{0,12}red/);
  assert.doesNotMatch(await inventory(), /ae2:energy_cell/);
  fixture.mark('blocked placement retains the item; after the neighbor update the plane places its charged, styled cell exactly once and it survives restart');

  await fixture.command(`setblock ${target} minecraft:air`);
  const shulker = 'minecraft:shulker_box[minecraft:container=[{slot:0,item:{id:"minecraft:emerald",count:3,components:{"minecraft:custom_data":{formation_marker:61}}}}],minecraft:custom_name={text:"Formation Box",color:"blue"},minecraft:lock={components:{"minecraft:custom_name":"Key"}}]';
  await offer(shulker, 'minecraft:shulker_box');
  await place('minecraft:shulker_box');
  assert.match(await block(), /^minecraft:shulker_box(?:\[|$)/);
  let box = await data();
  for (const marker of ['minecraft:emerald', 'formation_marker', 'Formation Box', 'lock']) assert(box.includes(marker), `placed shulker retains ${marker}`);
  assert.match(box, /color.{0,12}blue/);
  assert.match(box, /count.{0,12}3/);
  await restart();
  box = await data();
  for (const marker of ['minecraft:emerald', 'formation_marker', 'Formation Box', 'lock']) assert(box.includes(marker), `reloaded shulker retains ${marker}`);
  assert.match(box, /color.{0,12}blue/);
  assert.match(box, /count.{0,12}3/);
  assert.doesNotMatch(await inventory(), /minecraft:shulker_box/);
  fixture.mark('a placed shulker retains its nested component-bearing inventory, lock and styled name through restart');

  await fixture.command(`setblock ${target} minecraft:air`);
  await offer('ae2:crafting_monitor', 'ae2:crafting_monitor');
  await place('ae2:crafting_monitor');
  const monitor = await block();
  assert.match(monitor, /^ae2:crafting_monitor\[/);
  assert.match(monitor, /facing=west/);
  assert.match(monitor, /spin=0/);
  fixture.mark('an east-looking plane uses the source full-orientation strategy for a crafting monitor');

  await fixture.command(`setblock ${target} minecraft:air`);
  await offer('minecraft:torch', 'minecraft:torch');
  await place('minecraft:torch');
  assert.match(await block(), /^minecraft:(?:wall_)?torch(?:\[|$)/);
  fixture.mark('the plane uses the native BlockItem placement path for a supported torch');

  await fixture.command(`setblock ${target} minecraft:air`);
  await fixture.command(`setblock ${below} minecraft:farmland[moisture=7]`);
  await offer('minecraft:wheat_seeds', 'minecraft:wheat_seeds');
  await place('minecraft:wheat_seeds');
  assert.match(await block(), /^minecraft:wheat\[/);
  fixture.mark('a seed places its crop using the actual supporting farmland');

  await fixture.command(`setblock ${target} minecraft:air`);
  await fixture.command(`setblock ${below} minecraft:stone`);
  await offer('minecraft:oak_door', 'minecraft:oak_door');
  await place('minecraft:oak_door');
  assert.match(await block(), /^minecraft:oak_door\[.*half=lower/);
  assert.match(await block(above), /^minecraft:oak_door\[.*half=upper/);
  await restart();
  assert.match(await block(), /^minecraft:oak_door\[.*half=lower/);
  assert.match(await block(above), /^minecraft:oak_door\[.*half=upper/);
  assert.doesNotMatch(await inventory(), /minecraft:oak_door/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('one physical door item places both halves and both survive restart without an extra item');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
