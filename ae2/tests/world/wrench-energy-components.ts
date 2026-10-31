/** Wrenching an AE2 energy cell retains the source energy and rich name. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('wrench-energy-components', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 5, y = Math.ceil(py) + 4, z = Math.floor(pz) + 5;
  const support = `${x} ${y} ${z}`, cell = `${x} ${y + 1} ${z}`;
  const blockIs = (id: string) => fixture.command(`execute if block ${cell} ${id}`);
  await fixture.command(`fill ${x - 3} ${y - 1} ${z - 3} ${x + 3} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`fill ${x - 3} ${y} ${z - 3} ${x + 3} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`setblock ${support} minecraft:stone`);
  await fixture.command(`tp @s ${x + 0.5} ${y + 1} ${z + 2.5}`);
  await fixture.command('clear @s');
  await fixture.command('gamemode survival');
  await fixture.replace('hotbar.0', 'ae2:energy_cell[ae2:stored_energy=1234.0d,minecraft:custom_name={text:"Battery",color:"red"}]');
  await fixture.select(0, 'ae2:energy_cell');
  await fixture.raw(`interact ${support} up`);
  await until(() => blockIs('ae2:energy_cell'), result => result.chat?.includes('Test passed'), 'source-component energy cell placed', 30000);
  assert.equal((await fixture.ae(`power ${cell} east`)).energy, 1234);

  await fixture.replace('hotbar.1', 'ae2:certus_quartz_wrench');
  await fixture.select(1, 'ae2:certus_quartz_wrench');
  await fixture.raw('key shift down');
  try { await fixture.raw(`interact ${cell} south`); }
  finally { await fixture.raw('key shift up'); }
  await until(() => blockIs('minecraft:air'), result => result.chat?.includes('Test passed'), 'wrench removed the real cell', 30000);
  const inv = await until(() => fixture.raw('inventory'), view => view.hotbar?.some((item: any) => item?.item === 'ae2:energy_cell'), 'recovered physical cell in player inventory');
  const slot = inv.hotbar.findIndex((item: any) => item?.item === 'ae2:energy_cell');
  await fixture.select(slot, 'ae2:energy_cell');
  const recovered = JSON.stringify(await fixture.command('data get entity @s SelectedItem'));
  assert.match(recovered, /ae2:stored_energy.{0,12}1234/);
  assert.match(recovered, /color.{0,12}red/);
  fixture.mark('wrench disassembly returns the same stored energy and styled name as direct components');

  await fixture.raw(`interact ${support} up`);
  await until(() => blockIs('ae2:energy_cell'), result => result.chat?.includes('Test passed'), 'recovered cell placed', 30000);
  assert.equal((await fixture.ae(`power ${cell} east`)).energy, 1234);
  await fixture.restart();
  assert.equal((await fixture.ae(`power ${cell} east`)).energy, 1234);
  for (const [input, expected] of [['250000.0d', 200000], ['-4.0d', 0], ['0.0000005d', 0]] as const) {
    await fixture.command(`setblock ${cell} minecraft:air`);
    await fixture.replace('hotbar.0', `ae2:energy_cell[ae2:stored_energy=${input}]`);
    await fixture.select(0, 'ae2:energy_cell');
    await fixture.raw(`interact ${support} up`);
    await until(() => blockIs('ae2:energy_cell'), result => result.chat?.includes('Test passed'), 'boundary-valued cell placed', 30000);
    assert.equal((await fixture.ae(`power ${cell} east`)).energy, expected, `source setStored boundary for ${input}`);
  }
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the recovered cell restores its energy when placed and survives restart');
  fixture.mark('direct energy components clamp at capacity and round negative or sub-minimum values to zero');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
