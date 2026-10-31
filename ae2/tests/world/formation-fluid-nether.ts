/** The source consumes water without a block in an ultrawarm dimension. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('formation-fluid-nether', mode);
try {
  await fixture.start();
  await fixture.command('execute in minecraft:the_nether run tp @s 0.5 100 0.5');
  const at = (x: number, y = 100, z = 0) => `${x} ${y} ${z}`;
  await until(async () => (await fixture.command('execute if loaded 3 100 3')).chat ?? [], lines => lines.includes('Test passed'), 'Nether fixture chunk loaded', 30000);
  await fixture.command(`fill ${at(-3, 99, -3)} ${at(4, 99, 3)} minecraft:stone`);
  await fixture.command(`fill ${at(-3, 100, -3)} ${at(4, 104, 3)} minecraft:air`);
  await fixture.command('clear @s');
  await fixture.command('tick freeze');
  await fixture.command(`setblock ${at(-1)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${at(0)} ae2:cable_bus`);
  await fixture.replace('hotbar.0', 'ae2:formation_plane');
  await fixture.select(0, 'ae2:formation_plane');
  assert.equal((await fixture.ae(`grid part ${at(0)} east`)).attached, true);
  await until(() => fixture.ae(`grid status ${at(0)}`), s => s.online && s.powered, 'Nether plane online', 30000);
  const block = async () => (await fixture.raw(`getblock ${at(1)}`)).block as string;
  const insert = async (bucket: string) => {
    await fixture.replace('hotbar.1', bucket);
    await fixture.select(1, bucket);
    assert.equal((await fixture.ae(`fluid grid insert ${at(0)}`)).inserted_mb, 1000);
    assert.match(JSON.stringify(await fixture.command('data get entity @s SelectedItem')), /minecraft:bucket/);
  };
  await insert('minecraft:water_bucket');
  assert.match(await block(), /^minecraft:air(?:\[|$)/, 'water evaporates and leaves the target unchanged');
  await insert('minecraft:water_bucket');
  assert.match(await block(), /^minecraft:air(?:\[|$)/, 'repeated evaporation remains an accepted physical transfer');
  await insert('minecraft:lava_bucket');
  assert.match(await block(), /^minecraft:lava(?:\[|$)/, 'lava remains placeable in the same dimension');
  await fixture.restart();
  assert.match(await block(), /^minecraft:lava(?:\[|$)/, 'the actual lava block survives restart');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('two physical water buckets evaporate without blocks; a real lava source persists after restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
