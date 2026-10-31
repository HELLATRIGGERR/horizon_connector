/** Provider source components restore real patterns only by spending blanks. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('provider-memory-components', mode);
const text = (reply: any) => JSON.stringify(reply.chat ?? []);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const provider = `${x} ${y} ${z}`;
  await fixture.command(`fill ${x - 2} ${y} ${z - 2} ${x + 2} ${y + 3} ${z + 2} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 2} ${x + 2} ${y - 1} ${z + 2} minecraft:stone`);
  await fixture.command(`setblock ${provider} ae2:pattern_provider`);
  await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.1', 'ae2:blank_pattern');
  const pattern = '{id:"ae2:crafting_pattern",count:1,components:{"minecraft:custom_data":{ae2_lab_pattern:{output:{count:4,id:"minecraft:oak_planks"},fluid_substitutes:0b,grid:[{count:1,Slot:0b,id:"minecraft:oak_log"}],substitutes:0b}}}}';
  const card = `ae2:memory_card[ae2:exported_settings_source={translate:"block.ae2.pattern_provider"},ae2:exported_patterns=[{slot:0,item:${pattern}}],ae2:exported_settings={blocking_mode:"YES",pattern_access_terminal:"NO",lock_crafting_mode:"LOCK_WHILE_LOW"},ae2:exported_priority=42,ae2:exported_push_direction="north",ae2:exported_custom_name={text:"Factory",color:"red",italic:0b}]`;
  await fixture.replace('hotbar.0', card); await fixture.select(0, 'ae2:memory_card');
  await fixture.command(`tp @s ${x + 0.5} ${y} ${z + 1.5}`); await fixture.raw(`interact ${provider} south`);
  const data = () => fixture.command(`data get block ${provider}`);
  await until(data, reply => text(reply).includes('ae2:crafting_pattern') && /priority: 42/.test(text(reply)), 'source-only provider settings and physical pattern');
  assert.equal((await fixture.raw('inventory')).counts?.['ae2:blank_pattern'] ?? 0, 0, 'one physical blank was consumed');
  fixture.mark('source-only Provider pattern and settings import consumes one physical blank');

  const partCard = 'ae2:memory_card[ae2:exported_settings_source={translate:"item.ae2.cable_pattern_provider"},ae2:exported_patterns=[],ae2:exported_priority=7]';
  await fixture.replace('hotbar.0', partCard); await fixture.select(0, 'ae2:memory_card');
  await fixture.raw(`interact ${provider} south`);
  await until(data, reply => /priority: 7/.test(text(reply)), 'unlike source applies compatible priority');
  assert.match(text(await data()), /ae2:crafting_pattern/, 'generic import does not clear the physical pattern');
  const styled = 'ae2:memory_card[ae2:exported_settings_source={translate:"block.ae2.pattern_provider",color:"red"},ae2:exported_patterns=[],ae2:exported_priority=7,ae2:exported_custom_name="Foreign"]';
  await fixture.replace('hotbar.0', styled); await fixture.select(0, 'ae2:memory_card');
  await fixture.raw(`interact ${provider} south`);
  const preserved = text(await data());
  assert.match(preserved, /ae2:crafting_pattern/, 'styled foreign source keeps physical patterns');
  assert.match(preserved, /Factory/, 'styled foreign source does not replace the matching host name');
  await fixture.raw('key shift down');
  try { await fixture.raw(`interact ${provider} south`); } finally { await fixture.raw('key shift up'); }
  const exported = await until(() => fixture.command('data get entity @s SelectedItem'), reply => ['ae2:exported_patterns','ae2:exported_settings','ae2:exported_priority','ae2:exported_push_direction','ae2:exported_custom_name'].every(key => text(reply).includes(key)), 'Provider save emits source components');
  assert.match(text(exported), /Factory/); assert.match(text(exported), /color.*red/, 'rich source name survives import and export');
  await fixture.restart();
  assert.match(text(await data()), /ae2:crafting_pattern/);
  assert.match(text(await data()), /priority: 7/);
  assert.match(text(await data()), /Factory/); assert.match(text(await data()), /color.*red/);
  assert.equal((await fixture.raw('inventory')).counts?.['ae2:blank_pattern'] ?? 0, 0);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('styled and cross-source imports preserve patterns and rich names through export and restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
