/** Source-form Memory Cards on physical Storage and Energy Level Emitters. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('emitter-memory-components', mode);
const text = (reply: any) => JSON.stringify(reply.chat ?? []);

try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const at = (dx: number) => `${x + dx} ${y} ${z}`;
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 5} ${y + 3} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 5} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command('clear @s');
  for (const [dx, part] of [[0, 'ae2:level_emitter'], [3, 'ae2:energy_level_emitter']] as const) {
    await fixture.command(`setblock ${at(dx)} ae2:cable_bus`);
    await fixture.replace('hotbar.0', part); await fixture.select(0, part);
    assert.equal((await fixture.ae(`grid part ${at(dx)} south`)).attached, true);
  }
  // Survival's eye is above this narrow part. Give the south face enough
  // horizontal distance for the ordinary aim ray to hit it, not its top.
  await fixture.command(`tp @s ${x + 0.5} ${y} ${z + 2.5}`);
  await fixture.command('gamemode survival');
  await fixture.replace('hotbar.1', 'ae2:fuzzy_card');
  const card = 'ae2:memory_card[ae2:exported_settings_source={translate:"item.ae2.level_emitter"},ae2:exported_settings={redstone_emitter:"LOW_SIGNAL",fuzzy_mode:"PERCENT_75",craft_via_redstone:"YES"},ae2:exported_level_emitter_value=42L,ae2:exported_custom_name="Source emitter",ae2:exported_config_inv=[{"#t":"ae2:i",id:"minecraft:diamond","#":1L}],ae2:exported_upgrades=[{id:"ae2:fuzzy_card",count:1}]]';
  await fixture.replace('hotbar.0', card); await fixture.select(0, 'ae2:memory_card');
  await fixture.raw(`interact ${at(0)} south`);
  const part = () => fixture.command(`data get block ${at(0)} ae2_lab_config.parts`);
  const configured = await until(part, r => {
    const v = text(r);
    return /emitter_threshold:\s*42L/.test(v) && /emitter_low:\s*1b/.test(v)
      && v.includes('fuzzy_mode') && v.includes('percent_75') && v.includes('minecraft:diamond') && v.includes('ae2:fuzzy_card');
  }, 'source-only settings, filter and physical upgrade on Storage Level Emitter');
  assert.match(text(configured), /emitter_craft_via_redstone:\s*1b/);
  await until(() => fixture.raw('inventory'), v => !v.hotbar?.some((s: any) => s?.item === 'ae2:fuzzy_card'), 'the physical Fuzzy Card leaves the player inventory');
  fixture.mark('source-only Storage Level Emitter card imports settings and filter while moving the actual upgrade');

  await fixture.raw('key shift down');
  try { await fixture.raw(`interact ${at(0)} south`); } finally { await fixture.raw('key shift up'); }
  const saved = await until(() => fixture.command('data get entity @s SelectedItem'), r => {
    const v = text(r);
    return v.includes('ae2:exported_config_inv') && v.includes('ae2:exported_level_emitter_value') && v.includes('ae2:exported_upgrades');
  }, 'physical save writes source-form Memory Card components');
  assert.match(text(saved), /minecraft:diamond/); assert.match(text(saved), /ae2:fuzzy_card/);
  fixture.mark('saving the real emitter emits its source-form configuration and upgrade components');

  await fixture.command(`tp @s ${x + 3.5} ${y} ${z + 2.5}`);
  await fixture.raw(`interact ${at(3)} south`);
  const energy = await until(() => fixture.command(`data get block ${at(3)} ae2_lab_config.parts`), r => /emitter_low:\s*1b/.test(text(r)), 'cross-kind generic redstone setting');
  assert(!/emitter_threshold:\s*42L/.test(text(energy)), 'a different emitter type does not import the source-specific threshold');
  fixture.mark('a different emitter receives the compatible generic setting without taking its threshold');

  await fixture.restart();
  const reloaded = await until(part, r => /emitter_threshold:\s*42L/.test(text(r)) && text(r).includes('ae2:fuzzy_card'), 'emitter configuration after restart');
  assert.match(text(reloaded), /minecraft:diamond/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the configured emitter, physical upgrade and filter survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
