/** Structured Memory Card names survive machine and part import/export. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('memory-card-rich-names', mode);
const text = (reply: any) => JSON.stringify(reply.chat ?? []);
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const at = (dx: number) => `${x+dx} ${y} ${z}`;
  await fixture.command(`fill ${x-2} ${y} ${z-3} ${x+5} ${y+3} ${z+3} minecraft:air`);
  await fixture.command(`fill ${x-2} ${y-1} ${z-3} ${x+5} ${y-1} ${z+3} minecraft:stone`);
  await fixture.command('clear @s'); await fixture.command(`setblock ${at(0)} ae2:interface`); await fixture.command(`setblock ${at(3)} ae2:cable_bus`);
  await fixture.replace('hotbar.0', 'ae2:level_emitter'); await fixture.select(0, 'ae2:level_emitter');
  assert.equal((await fixture.ae(`grid part ${at(3)} south`)).attached, true);
  const name = '{translate:"item.minecraft.diamond",color:"red",extra:[{text:"!",bold:1b}]}';
  const use = async (dx: number, source: string, threshold = '') => {
    const card = `ae2:memory_card[ae2:exported_settings_source={translate:"${source}"},ae2:exported_custom_name=${name}${threshold}]`;
    await fixture.replace('hotbar.0', card); await fixture.select(0, 'ae2:memory_card');
    await fixture.command(`tp @s ${x+dx+.5} ${y} ${z+2.5}`); await fixture.raw(`interact ${at(dx)} south`);
  };
  const save = async (dx: number) => {
    await fixture.command(`tp @s ${x+dx+.5} ${y} ${z+2.5}`);
    await fixture.raw('key shift down');
    try { await fixture.raw(`interact ${at(dx)} south`); } finally { await fixture.raw('key shift up'); }
    const item = await until(() => fixture.command('data get entity @s SelectedItem'), r => text(r).includes('ae2:exported_custom_name'), 'source Memory Card name exported');
    for (const expected of ['item.minecraft.diamond', 'color', 'red', 'extra', 'bold']) assert(text(item).includes(expected), text(item));
  };
  await use(0, 'block.ae2.interface'); await save(0);
  fixture.mark('a styled translatable name imports and re-exports through the physical Interface');
  await use(3, 'item.ae2.level_emitter', ',ae2:exported_level_emitter_value=42L'); await save(3);
  fixture.mark('the same structured name imports and re-exports through the physical cable part');
  const styledSource = 'ae2:memory_card[ae2:exported_settings_source={translate:"item.ae2.level_emitter",color:"red"},ae2:exported_custom_name="wrong name",ae2:exported_level_emitter_value=987L,ae2:exported_settings={redstone_emitter:"LOW_SIGNAL"}]';
  await fixture.replace('hotbar.0', styledSource); await fixture.select(0, 'ae2:memory_card');
  await fixture.raw(`interact ${at(3)} south`);
  await until(() => fixture.command(`data get block ${at(3)} ae2_lab_config.parts`), r => /emitter_threshold:\s*42L/.test(text(r)) && /emitter_low:\s*1b/.test(text(r)), 'styled foreign source imports generic setting only');
  await save(3);
  fixture.mark('a styled source description imports generic settings without changing the matching-only name or threshold');
  const listSource = `ae2:memory_card[ae2:exported_settings_source=[{translate:"item.ae2.level_emitter"}],ae2:exported_custom_name=${name},ae2:exported_level_emitter_value=55L]`;
  await fixture.replace('hotbar.0', listSource); await fixture.select(0, 'ae2:memory_card'); await fixture.raw(`interact ${at(3)} south`);
  await until(() => fixture.command(`data get block ${at(3)} ae2_lab_config.parts`), r => /emitter_threshold:\s*55L/.test(text(r)), 'singleton-list source description matches its component');
  fixture.mark('a singleton component list matches the same source item description');
  await fixture.restart(); await fixture.select(0, 'ae2:memory_card');
  await save(0); await save(3); fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('both machine and part names retain their source text components after restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
