/** Source-only stonecutting/smithing patterns execute through physical providers and assemblers. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';
const fixture = new WorldFixture('source-special-patterns');
const name = '{text:"Source blade",color:"gold",italic:1b}';
const cell = `ae2:item_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:stone","#":3L},{"#t":"ae2:i",id:"minecraft:netherite_upgrade_smithing_template","#":1L},{"#t":"ae2:i",id:"minecraft:diamond_sword",components:{"minecraft:custom_name":${name}},"#":1L},{"#t":"ae2:i",id:"minecraft:netherite_ingot","#":1L}]]`;
const stone = 'ae2:stonecutting_pattern[ae2:encoded_stonecutting_pattern={input:{id:"minecraft:stone",count:1},output:{id:"minecraft:stone_slab",count:2},canSubstitute:0b,recipeId:"minecraft:stone_slab_from_stone_stonecutting"}]';
const smith = `ae2:smithing_table_pattern[ae2:encoded_smithing_table_pattern={template:{id:"minecraft:netherite_upgrade_smithing_template",count:1},base:{id:"minecraft:diamond_sword",count:1,components:{"minecraft:custom_name":${name}}},addition:{id:"minecraft:netherite_ingot",count:1},resultItem:{id:"minecraft:netherite_sword",count:1,components:{"minecraft:custom_name":${name}}},canSubstitute:0b,recipeId:"minecraft:netherite_sword_smithing"}]`;
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), provider = at(3), assembler = at(3, 0, -1), cpu = at(2, 1);
  await fixture.command(`tp @s ${x + 2.5} ${y + 1} ${z + 0.5}`);
  for (const dx of [-2, 6]) for (const dz of [-3, 3]) await until(() => fixture.command(`execute if loaded ${at(dx, 0, dz)}`), reply => reply.chat?.some((line: string) => line.includes('Test passed')), 'loaded special-pattern build corner', 30000);
  await fixture.command('clear @s'); await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 5, 3)} minecraft:air`); await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 3)} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'], [cpu, '64k_crafting_storage'], [provider, 'pattern_provider'], [assembler, 'molecular_assembler']]) await fixture.command(`setblock ${pos} ae2:${id}`);
  await fixture.command(`item replace block ${drive} container.0 with ${cell}`);
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal'); assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const status = () => fixture.ae(`grid status ${drive}`);
  const ready = () => until(status, view => view.online && view.powered, 'special-pattern network powered', 30000);
  const count = (view: any, id: string) => (view.items ?? []).filter((row: any) => row.item === id).reduce((n: number, row: any) => n + row.count, 0);
  await ready(); for (const pos of [provider, assembler]) await until(() => fixture.ae(`grid status ${pos}`), view => view.online && view.powered, 'special host online', 30000);
  await fixture.replace('hotbar.0', stone); await fixture.replace('hotbar.1', smith); await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 5.5} ${y} ${z + 0.5}`); await fixture.raw(`interact ${provider} east`);
  for (const id of ['ae2:stonecutting_pattern', 'ae2:smithing_table_pattern']) {
    const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === id), 'source special pattern in inventory');
    await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === id).slot} shift`);
    await until(() => fixture.raw('inventory'), view => !view.counts?.[id], 'physical special pattern installed');
  }
  await fixture.raw('close');
  const craft = async (id: string) => {
    await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
    const screen = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === id), 'source special output craftable');
    await fixture.raw(`click-slot ${screen.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === id).slot} shift`);
    await until(() => fixture.raw('screen'), view => JSON.stringify(view.title).includes('AE2 Craft Amount'), 'special amount'); await fixture.raw('click-slot 302');
    const confirm = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title).includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), 'special confirmation', 90000);
    assert.doesNotMatch(JSON.stringify(confirm), /Missing materials|craft planning:/); await fixture.raw('click-slot 302'); await fixture.raw('close');
    return await until(ready, view => count(view, id) > 0, 'physical special assembly completes', 90000);
  };
  const cut = await craft('minecraft:stone_slab'); assert.equal(count(cut, 'minecraft:stone_slab'), 2); assert.equal(count(cut, 'minecraft:stone'), 2);
  fixture.mark('a source-only named stonecutting pattern consumes one stone and makes two slabs in the physical assembler');
  const forged = await craft('minecraft:netherite_sword');
  for (const id of ['minecraft:diamond_sword', 'minecraft:netherite_ingot', 'minecraft:netherite_upgrade_smithing_template']) assert.equal(count(forged, id), 0);
  assert.equal(count(forged, 'minecraft:netherite_sword'), 1);
  const checkName = async () => { const saved = (await fixture.command(`data get block ${drive}`)).chat?.join('\n') ?? ''; assert.match(saved, /Source blade/); assert.match(saved, /gold/); assert.match(saved, /italic/); };
  await checkName(); await fixture.restart(); const restored = await ready(); assert.equal(count(restored, 'minecraft:stone_slab'), 2); assert.equal(count(restored, 'minecraft:netherite_sword'), 1); await checkName();
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('source smithing consumes all three physical ingredients and retains the base name/style after restart'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; } finally { await fixture.close(); }
