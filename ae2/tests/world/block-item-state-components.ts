/** BlockItem applies valid state components and retains hive occupants. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('block-item-state-components', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 5, y = Math.ceil(py) + 4, z = Math.floor(pz) + 5;
  const support = `${x} ${y} ${z}`, target = `${x} ${y + 1} ${z}`;
  await fixture.command(`fill ${x - 3} ${y - 1} ${z - 3} ${x + 3} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`fill ${x - 3} ${y} ${z - 3} ${x + 3} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`setblock ${support} minecraft:stone`);
  await fixture.command(`tp @s ${x + 0.5} ${y + 1} ${z + 2.5}`);
  await fixture.command('clear @s');
  await fixture.command('gamemode survival');
  await fixture.replace('hotbar.0', 'minecraft:beehive[minecraft:bees=[{entity_data:{id:"minecraft:bee",CustomName:"Queen"},ticks_in_hive:0,min_ticks_in_hive:10000}],minecraft:block_state={honey_level:"4",facing:"invalid",unknown:"ignored"}]');
  await fixture.select(0, 'minecraft:beehive');
  await fixture.raw(`interact ${support} up`);
  await until(() => fixture.command(`execute if block ${target} minecraft:beehive[honey_level=4]`), reply => reply.chat?.includes('Test passed'), 'real item placement applies its valid honey level', 30000);
  const entity = JSON.stringify(await fixture.command(`data get block ${target}`));
  assert.match(entity, /Queen/);
  fixture.mark('the placed hive has honey level four and its real resident bee despite unrelated invalid properties');
  await fixture.restart();
  assert((await fixture.command(`execute if block ${target} minecraft:beehive[honey_level=4]`)).chat?.includes('Test passed'));
  assert.match(JSON.stringify(await fixture.command(`data get block ${target}`)), /Queen/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the applied state and bee survive the ordinary restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
