/** Source ArmorStandItem.useOn through a real powered Formation Plane. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('formation-armor-stand', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 5, z = Math.floor(pz) + 6;
  const at = (dx = 0, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const host = at(), target = at(1);
  const block = async () => String((await fixture.raw(`getblock ${target}`)).block);
  const inventory = async () => JSON.stringify(await fixture.command('data get entity @s Inventory'));
  const stand = async () => JSON.stringify(await fixture.command('data get entity @e[type=minecraft:armor_stand,limit=1,sort=nearest]'));
  const equipment = async () => JSON.stringify(await fixture.command('data get entity @e[type=minecraft:armor_stand,limit=1,sort=nearest] equipment'));
  const count = async () => JSON.stringify(await fixture.command('execute if entity @e[type=minecraft:armor_stand,distance=..16]'));
  const ready = async () => {
    await fixture.stepServer(100);
    await until(() => fixture.ae(`grid part-status ${host} east`), state => state.online && state.powered, 'powered formation part', 30000);
  };
  const offer = async (spec: string) => {
    // Client inventory packets require live server iterations; hold again
    // before issuing the authoritative grid operation.
    await fixture.hold(false);
    try {
      await fixture.replace('hotbar.1', spec);
      await fixture.select(1, 'minecraft:armor_stand');
    } finally { await fixture.hold(); }
    await ready();
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

  const item = 'minecraft:armor_stand[minecraft:custom_name={text:"Guardian",color:"gold"},minecraft:entity_data={id:"minecraft:armor_stand",UUID:[I;1,2,3,4],Small:1b,ShowArms:1b,CustomNameVisible:1b,HandItems:[{id:"minecraft:emerald",count:2},{}],ArmorItems:[{},{},{},{id:"minecraft:diamond_helmet",count:1}]}]';
  await fixture.command(`setblock ${target} minecraft:stone`);
  await offer(item);
  assert.equal((await fixture.ae(`grid deposit ${host}`)).inserted, 0, 'a blocked target keeps the source item');
  assert.match(await inventory(), /Guardian/);
  await fixture.command(`setblock ${target} minecraft:air`);
  assert.equal((await fixture.ae(`grid deposit ${host}`)).inserted, 1, 'one Armor Stand item is consumed');
  assert.match(await block(), /^minecraft:air(?:\[|$)/, 'the item spawns an entity without placing a block');
  assert.doesNotMatch(await inventory(), /minecraft:armor_stand/, 'the consumed item is absent from the player');
  assert.match(await count(), /Test passed[^\d]*1\b/, 'preview and commit leave exactly one stand');
  let data = await stand();
  for (const field of ['Guardian', 'gold', 'Small', 'ShowArms', 'minecraft:emerald', 'minecraft:diamond_helmet']) {
    assert(data.includes(field), `spawned entity retains ${field}`);
  }
  assert.match(data, /Small.{0,8}1b/);
  assert.match(data, /ShowArms.{0,8}1b/);
  const uuid = data.match(/UUID.{0,8}(\[I;[^\]]+\])/);
  assert(uuid && uuid[1] !== '[I;1,2,3,4]', 'source EntityData cannot replace the new entity UUID');
  assert.match(await equipment(), /mainhand.{0,90}minecraft:emerald/);
  assert.match(await equipment(), /head.{0,90}minecraft:diamond_helmet/);
  assert.match(data, /Rotation.{0,20}90(?:\.0)?f/, 'east-looking plane supplies the source yaw');
  fixture.mark('one styled and equipped Armor Stand entity is created once; the item and replaceable block have correct custody');

  await offer('minecraft:armor_stand[minecraft:custom_name={text:"Spare"}]');
  assert.equal((await fixture.ae(`grid deposit ${host}`)).inserted, 0, 'a stand obstructing the source collision box rejects a second item');
  assert.match(await inventory(), /Spare/, 'the rejected second item remains with the player');
  assert.match(await count(), /Test passed[^\d]*1\b/);
  await fixture.hold(false);
  await fixture.restart();
  await fixture.hold();
  await ready();
  assert.match(await count(), /Test passed[^\d]*1\b/, 'the spawned stand survives ordinary restart exactly once');
  data = await stand();
  assert.equal(data.match(/UUID.{0,8}(\[I;[^\]]+\])/)?.[1], uuid[1], 'the entity identity survives restart');
  for (const field of ['Guardian', 'gold', 'Small', 'ShowArms', 'minecraft:emerald', 'minecraft:diamond_helmet']) {
    assert(data.includes(field), `reloaded entity retains ${field}`);
  }
  assert.match(await equipment(), /mainhand.{0,90}minecraft:emerald/);
  assert.match(await equipment(), /head.{0,90}minecraft:diamond_helmet/);
  assert.match(await inventory(), /Spare/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('entity collision rejects another item and both entity data and the rejected item survive restart');

  // EntityType.getYOffset also tests collidable entities immediately below
  // the clicked block. A boat supports the stand even though no block does.
  await fixture.hold(false);
  try { await fixture.select(1, 'minecraft:armor_stand'); }
  finally { await fixture.hold(); }
  await fixture.command('kill @e[type=minecraft:armor_stand,distance=..16]');
  await fixture.command(`setblock ${at(1, -1)} minecraft:air`);
  await fixture.command(`summon minecraft:oak_boat ${x + 1.5} ${y - 0.75} ${z + 0.5}`);
  assert.match(JSON.stringify(await fixture.command('execute if entity @e[type=minecraft:oak_boat,distance=..16]')), /Test passed[^\d]*1\b/);
  assert.equal((await fixture.ae(`grid deposit ${host}`)).inserted, 1);
  const position = JSON.stringify(await fixture.command('data get entity @e[type=minecraft:armor_stand,limit=1,sort=nearest] Pos[1]'));
  const value = position.match(/(-?\d+(?:\.\d+)?)d/);
  assert(value && Math.abs(Number(value[1]) - (y - 0.1875)) < 1e-5, `stand rests on boat collision box: ${position}`);
  assert.doesNotMatch(await inventory(), /minecraft:armor_stand/);
  fixture.mark('the source vertical placement probe supports the consumed stand on a boat below the clicked block');

  // An upward-looking plane supplies DOWN as its clicked face, and
  // ArmorStandItem.useOn rejects that face before testing collision.
  await fixture.command(`setblock ${target} minecraft:stone`);
  await fixture.hold(false);
  try {
    await fixture.replace('hotbar.2', 'ae2:formation_plane');
    await fixture.select(2, 'ae2:formation_plane');
  } finally { await fixture.hold(); }
  assert.equal((await fixture.ae(`grid part ${host} up`)).attached, true);
  await fixture.stepServer(100);
  await until(() => fixture.ae(`grid part-status ${host} up`), state => state.online && state.powered, 'upward formation part', 30000);
  await offer('minecraft:armor_stand[minecraft:custom_name={text:"Upward Spare"}]');
  assert.match(String((await fixture.raw(`getblock ${at(0, 1)}`)).block), /^minecraft:air(?:\[|$)/);
  assert.equal((await fixture.ae(`grid deposit ${host}`)).inserted, 0, 'the source rejects the downward clicked face');
  assert.match(await inventory(), /Upward Spare/);
  fixture.mark('an upward-facing plane rejects the downward clicked face and keeps the item');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
