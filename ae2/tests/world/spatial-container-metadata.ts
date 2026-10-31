/** Spatial exchange carries the actual container's source metadata. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('spatial-container-metadata', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px), y = Math.ceil(py) + 8, z = Math.floor(pz);
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const port = at(-1, -1, 1), box = at(1, 1, 1);
  const data = async (pos: string) => JSON.stringify(await fixture.command(`data get block ${pos}`));
  await fixture.command(`fill ${at(-5, -2, -4)} ${at(7, 5, 7)} minecraft:air`);
  await fixture.command(`fill ${at(-5, -2, -4)} ${at(7, -2, 7)} minecraft:smooth_stone`);
  for (const [dx, dz] of [[0, 0], [0, 3], [3, 0], [3, 3]]) for (let dy = 0; dy < 4; dy++) {
    await fixture.command(`setblock ${at(dx, dy, dz)} ae2:spatial_pylon`);
  }
  for (let i = 0; i <= 3; i++) for (const [dx, dz] of [[i, 0], [i, 3], [0, i], [3, i]]) {
    await fixture.command(`setblock ${at(dx, -1, dz)} ae2:cable_bus`);
  }
  await fixture.command(`setblock ${at(-1, -1, 0)} ae2:controller`);
  await fixture.command(`setblock ${at(-2, -1, 0)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${port} ae2:spatial_io_port`);
  await fixture.command(`item replace block ${port} container.0 with ae2:spatial_storage_cell_2`);
  await until(() => fixture.ae(`grid status ${port}`), s => s.online && s.powered, 'powered spatial port', 30000);
  await fixture.command(`setblock ${box} minecraft:shulker_box`);
  await fixture.command(`data merge block ${box} {CustomName:{text:"Cargo",color:"red"},lock:{components:{"minecraft:custom_name":"Key"}},Items:[{Slot:0b,id:"minecraft:emerald",count:7,components:{"minecraft:custom_data":{spatial_marker:53}}}]}`);
  const before = await data(box);
  assert.match(before, /spatial_marker/);
  assert.match(before, /lock/);
  assert.match(before, /color.{0,12}red/);
  await fixture.command(`tp @s ${x - 3.5} ${y} ${z - 2.5}`);
  const first = await fixture.ae(`spatial transition ${port}`);
  assert.deepEqual(first.size, [2, 2, 2]);
  assert((await fixture.command(`execute if block ${box} minecraft:air`)).chat?.includes('Test passed'));
  await fixture.restart();
  await fixture.command(`item replace block ${port} container.0 from block ${port} container.1`);
  await fixture.command(`item replace block ${port} container.1 with minecraft:air`);
  await until(() => fixture.ae(`grid status ${port}`), s => s.online && s.powered, 'spatial port restored', 30000);
  assert.equal((await fixture.ae(`spatial transition ${port}`)).plot, first.plot);
  assert((await fixture.command(`execute if block ${box} minecraft:shulker_box`)).chat?.includes('Test passed'));
  const after = await data(box);
  assert.match(after, /spatial_marker/);
  assert.match(after, /color.{0,12}red/);
  assert.match(after, /lock/);
  fixture.mark('a spatially exchanged shulker retains its physical item, styled name and lock after restart');
  await fixture.restart();
  const saved = await data(box);
  assert.match(saved, /spatial_marker/);
  assert.match(saved, /color.{0,12}red/);
  assert.match(saved, /lock/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the restored container metadata survives the following ordinary save and restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
