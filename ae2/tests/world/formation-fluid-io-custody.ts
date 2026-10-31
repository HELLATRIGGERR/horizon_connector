/** A partial source cell transfers one complete bucket through an I/O Port. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('formation-fluid-io-custody', mode);
const cell = 'ae2:fluid_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":1377L}]]';
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 5, z = Math.floor(pz) + 6;
  const at = (dx = 0, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const host = at(), power = at(-1), port = at(0, 0, -1), target = at(1);
  const block = async () => (await fixture.raw(`getblock ${target}`)).block as string;
  const contents = async () => JSON.stringify(await fixture.command(`data get block ${port}`));
  await fixture.command(`fill ${at(-3, -1, -3)} ${at(4, -1, 3)} minecraft:stone`);
  await fixture.command(`fill ${at(-3, 0, -3)} ${at(4, 4, 3)} minecraft:air`);
  await fixture.command(`tp @s ${x + 1.5} ${y + 1} ${z - 2.5}`);
  await fixture.command('clear @s');
  await fixture.command(`setblock ${power} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${port} ae2:io_port`);
  await fixture.command(`setblock ${host} ae2:cable_bus`);
  await fixture.replace('hotbar.0', 'ae2:formation_plane');
  await fixture.select(0, 'ae2:formation_plane');
  assert.equal((await fixture.ae(`grid part ${host} east`)).attached, true);
  await until(() => fixture.ae(`grid status ${port}`), s => s.online && s.powered, 'I/O and plane grid online', 30000);
  await fixture.command('tick freeze');
  await fixture.hold();
  await fixture.command(`item replace block ${port} container.0 with ${cell}`);
  await fixture.stepServer(500);
  assert.match(await block(), /^minecraft:water(?:\[|$)/);
  const residual = await contents();
  assert.match(residual, /minecraft:water/);
  assert.match(residual, /377L/, 'the input cell retains the exact 377 mB remainder');
  assert.doesNotMatch(residual, /1377L/, 'the accepted bucket leaves physical source custody');
  fixture.mark('one full bucket is placed and exactly 377 mB remains in the physical I/O cell');

  await fixture.hold(false);
  await fixture.restart();
  await until(() => fixture.ae(`grid status ${port}`), s => s.online && s.powered, 'I/O grid restored', 30000);
  assert.match(await block(), /^minecraft:water(?:\[|$)/);
  assert.match(await contents(), /377L/, 'the remaining source fluid survives restart');
  // The server's vanilla tick freeze is runtime state and is reset by restart.
  // Keep the level-15 flowing fluid still long enough to inspect its placement.
  await fixture.command('tick freeze');
  await fixture.hold();
  await fixture.command(`setblock ${target} minecraft:air`);
  await fixture.stepServer(500);
  assert.match(await block(), /^minecraft:air(?:\[|$)/, 'a 377 mB remainder cannot form another source');
  assert.match(await contents(), /377L/, 'the sub-bucket remainder stays in source custody');
  // Replace the previously verified source cell as isolated fixture setup.
  // Water may have flowed during restart before the powered grid was ready;
  // remove its adjacent blocks so placing lava cannot immediately make stone.
  await fixture.command(`fill ${at(1, 0, -1)} ${at(3, 1, 1)} minecraft:air`);
  // FlowingFluid.defaultFluidState is falling with level one; its legacy
  // liquid-block state is level 15 rather than a still source block.
  const flowing = 'ae2:fluid_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:flowing_lava","#":1000L}]]';
  await fixture.command(`item replace block ${port} container.0 with ${flowing}`);
  await fixture.stepServer(500);
  assert.match(await block(), /^minecraft:lava\[.*level=15/, 'the flowing fluid key places its distinct default legacy state');
  assert.doesNotMatch(await contents(), /minecraft:flowing_lava/, 'the placed bucket left its physical source cell');
  await fixture.command(`setblock ${target} minecraft:air`);
  const variant = 'ae2:fluid_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water",components:{"minecraft:custom_data":{variant:"warm"}},"#":1000L}]]';
  await fixture.command(`item replace block ${port} container.0 with ${variant}`);
  await fixture.stepServer(500);
  assert.match(await block(), /^minecraft:air(?:\[|$)/, 'source FluidPlacementStrategy refuses patched fluid components');
  assert.match(await contents(), /variant/);
  assert.match(await contents(), /1000L/, 'rejected component-bearing fluid remains in the physical cell');
  // The old native cell representation can contain the two-byte network NBT
  // encoding of an empty compound. It represents an empty source patch.
  const emptyPatch = 'ae2:fluid_storage_cell_1k[minecraft:custom_data={ae2_fluid_cell:{version:1,entries:[{fluid:"minecraft:water",components:[B;10b,0b],amount:1000L}]}}]';
  await fixture.command(`item replace block ${port} container.0 with ${emptyPatch}`);
  await fixture.stepServer(500);
  assert.match(await block(), /^minecraft:water(?:\[|$)/, 'an encoded empty patch is accepted as unpatched water');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('partial custody survives restart; flowing lava and empty patches place, while a nonempty patch is refused');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
