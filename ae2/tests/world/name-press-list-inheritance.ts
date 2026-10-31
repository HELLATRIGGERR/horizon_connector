/** Component-list root style must survive the physical Name Press recipe. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('name-press-list-inheritance', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 3, z = Math.floor(pz) + 6;
  const machine = `${x} ${y} ${z}`, power = `${x + 1} ${y} ${z}`;
  const nbt = async () => JSON.stringify(await fixture.command(`data get block ${machine}`));
  const combined = (value: string) => /minecraft:custom_name.*color.*red.*extra.*tail.*Suffix.*text.*Red/.test(value);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 2} ${x + 3} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`fill ${x - 2} ${y} ${z - 2} ${x + 3} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`setblock ${power} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${machine} ae2:inscriber`);
  await until(() => fixture.ae(`grid status ${machine}`), s => s.online && s.powered, 'inscriber grid online', 30000);
  await fixture.command(`data merge block ${machine} {Items:[{Slot:0b,id:"ae2:name_press",count:1,components:{"ae2:name_press_name":[{text:"Red",color:"red"},"tail"]}},{Slot:1b,id:"ae2:name_press",count:1,components:{"ae2:name_press_name":"Suffix"}},{Slot:2b,id:"minecraft:diamond",count:1}]}`);
  await until(() => fixture.raw(`container ${machine}`), v => v.slots?.[3]?.item === 'minecraft:diamond', 'renamed physical output', 30000);
  const before = await until(nbt, combined, 'list root color applies to appended siblings', 30000);
  assert.match(before, /ae2:name_press_name/);
  await fixture.restart();
  await until(nbt, combined, 'list root and siblings persist after restart', 30000);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('list component retains its root style, siblings and physical output through restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
