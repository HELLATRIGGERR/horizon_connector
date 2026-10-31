/** Real source Name Press components and legacy native presses in an inscriber. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('name-press-components', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 3, z = Math.floor(pz) + 6;
  const machine = `${x} ${y} ${z}`, power = `${x + 1} ${y} ${z}`;
  const nbt = async () => JSON.stringify(await fixture.command(`data get block ${machine}`));
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 2} ${x + 3} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`fill ${x - 2} ${y} ${z - 2} ${x + 3} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`setblock ${power} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${machine} ae2:inscriber`);
  await until(() => fixture.ae(`grid status ${machine}`), s => s.online && s.powered, 'inscriber grid online', 30000);
  await fixture.command(`data merge block ${machine} {Items:[{Slot:0b,id:"ae2:name_press",count:1,components:{"ae2:name_press_name":"Alpha"}},{Slot:1b,id:"ae2:name_press",count:1,components:{"ae2:name_press_name":"Beta"}},{Slot:2b,id:"minecraft:diamond",count:1,components:{"minecraft:custom_data":{name_press_marker:7}}}]}`);
  await until(() => fixture.raw(`container ${machine}`), v => v.slots?.some((s: any) => s?.item === 'minecraft:diamond'), 'source-component renamed item', 30000);
  // The source component is a tree: its appended siblings are serialized in
  // `extra`, while the first literal remains in `text`. Inspect the output's
  // actual component rather than expecting the flattened display string.
  const combined = (value: string) => /minecraft:custom_name.*extra.*Beta.*text.*Alpha/.test(value) && /name_press_marker/.test(value);
  const first = await until(nbt, combined, 'combined physical custom name');
  assert.match(first, /ae2:name_press_name/);
  fixture.mark('two real AE2 name components rename a physical item while preserving its other components');
  await fixture.restart();
  const saved = await until(nbt, combined, 'renamed output after restart', 30000);
  assert.match(saved, /ae2:name_press_name/);
  fixture.mark('name components, surviving presses and output persist through restart');

  await fixture.command(`data merge block ${machine} {Items:[{Slot:0b,id:"ae2:name_press",count:1,components:{"minecraft:custom_data":{"ae2:name_press_name":"Legacy"}}},{Slot:2b,id:"minecraft:gold_ingot",count:1}]}`);
  await until(nbt, value => /minecraft:gold_ingot/.test(value) && /minecraft:custom_name.*Legacy/.test(value), 'legacy press names physical output', 30000);
  fixture.mark('a name press from an earlier native save remains usable');

  await fixture.command(`data merge block ${machine} {Items:[{Slot:0b,id:"ae2:name_press",count:1,components:{"ae2:name_press_name":{text:"Red",color:"red"}}},{Slot:1b,id:"ae2:name_press",count:1,components:{"ae2:name_press_name":{translate:"item.minecraft.diamond",bold:1b}}},{Slot:2b,id:"minecraft:iron_ingot",count:1}]}`);
  await until(() => fixture.raw(`container ${machine}`), v => v.slots?.[3]?.item === 'minecraft:iron_ingot', 'styled-name physical output', 30000);
  const styled = await until(nbt, value => /minecraft:custom_name.*color.*red.*translate.*item.minecraft.diamond/.test(value), 'styled and translatable output component', 30000);
  assert.match(styled, /minecraft:custom_name.*bold/);
  await fixture.restart();
  await until(nbt, value => /minecraft:custom_name.*color.*red.*translate.*item.minecraft.diamond/.test(value), 'structured name survives restart', 30000);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('styled and translatable component trees survive physical output and restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
