/** Source SetStockAmountMenu semantics for block and cable-part Interfaces. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('interface-stock-editor', 'server-thread');
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
const text = (reply: any) => JSON.stringify(reply.chat ?? []);
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const at = (dx: number) => `${x + dx} ${y} ${z}`, block = at(0), part = at(4);
  await fixture.command(`fill ${x-2} ${y} ${z-3} ${x+6} ${y+4} ${z+3} minecraft:air`);
  await fixture.command(`fill ${x-2} ${y-1} ${z-3} ${x+6} ${y-1} ${z+3} minecraft:stone`);
  await fixture.command(`setblock ${block} ae2:interface`); await fixture.command(`setblock ${part} ae2:cable_bus`); await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:cable_interface'); await fixture.select(0, 'ae2:cable_interface');
  assert.equal((await fixture.ae(`grid part ${part} south`)).attached, true);
  const card = 'ae2:memory_card[ae2:exported_settings_source={translate:"block.ae2.interface"},ae2:exported_config_inv=[{"#t":"ae2:i",id:"minecraft:diamond","#":3L},{"#t":"ae2:f",id:"minecraft:water","#":1000L},{"#t":"ae2:i",id:"minecraft:gold_ingot",components:{"minecraft:max_stack_size":5},"#":3L}]]';
  const data = (target: string, isPart: boolean) => fixture.command(`data get block ${target} ae2_lab_config${isPart ? '.parts[{face:"south"}]' : ''}`);
  for (const [target,dx,isPart] of [[block,0,false],[part,4,true]] as const) {
    await fixture.replace('hotbar.0', card); await fixture.select(0, 'ae2:memory_card');
    await fixture.command(`tp @s ${x+dx+.5} ${y} ${z+2.5}`); await fixture.raw(`interact ${target} south`);
    await until(() => data(target,isPart), reply => text(reply).includes('minecraft:diamond') && text(reply).includes('1000L'), 'source stock targets installed');
  }
  await fixture.command('gamemode survival @s'); await fixture.replace('hotbar.0', 'minecraft:stick');
  const open = async (target: string, dx: number) => {
    await fixture.select(0, 'minecraft:stick'); await fixture.command(`tp @s ${x+dx+.5} ${y} ${z+2.5}`); await fixture.raw(`interact ${target} south`);
    return until(() => fixture.raw('screen'), view => slot(view,309)?.item === 'minecraft:diamond', 'physical Interface menu');
  };
  const edit = async (index: number) => {
    await fixture.raw(`click-slot ${309+index} shift`);
    await until(() => fixture.raw('screen'), view => String(slot(view,300)?.components ?? '').includes('Stock +1') && String(slot(view,308)?.components ?? '').includes('Set stock'), 'stock amount editor');
  };
  for (const [target,dx,isPart] of [[block,0,false],[part,4,true]] as const) {
    await fixture.replace('hotbar.1', 'minecraft:diamond', 2); await open(target,dx);
    await fixture.raw('click-slot 1'); await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:diamond' && view.carried.count === 2, 'physical filter template on cursor');
    await fixture.raw('click-slot 309'); await until(() => fixture.raw('screen'), view => slot(view,309)?.count === 5, 'matching key increases stock by held amount');
    await fixture.raw('click-slot 309 right'); await until(() => fixture.raw('screen'), view => slot(view,309)?.count === 3, 'matching key decreases stock by held amount');
    await fixture.raw('click-slot 1'); await until(() => fixture.raw('inventory'), view => !view.carried && view.counts?.['minecraft:diamond'] === 2, 'physical filter template returned');
    await edit(0); await fixture.raw('type =2*3');
    assert.match(text(await data(target,isPart)), /count:\\?\s*3\b/, 'typing alone does not commit');
    await fixture.raw('ui-key Enter');
    await until(() => data(target,isPart), reply => /count:\\?\s*6\b/.test(text(reply)), 'item stock expression committed');
    await until(() => fixture.raw('screen'), view => slot(view,309)?.item === 'minecraft:diamond' && slot(view,309)?.count === 6, 'returned to physical Interface');
    await edit(1); await fixture.raw('type 1.5'); await fixture.raw('ui-key Enter');
    await until(() => data(target,isPart), reply => /1500L/.test(text(reply)), 'decimal fluid unit committed as 1500 mB');
    await until(() => fixture.raw('screen'), view => slot(view,309)?.item === 'minecraft:diamond', 'returned after fluid edit');
    await edit(0); await fixture.raw('type 65'); await fixture.raw('ui-key Enter');
    await fixture.command(`data get block ${target} ae2_lab_config`);
    assert.ok(String(slot(await fixture.raw('screen'),308)?.components ?? '').includes('Set stock'), 'over-limit amount cannot be confirmed');
    assert.match(text(await data(target,isPart)), /count:\\?\s*6\b/);
    await fixture.raw('click-slot 309');
    await until(() => fixture.raw('screen'), view => slot(view,309)?.item === 'minecraft:diamond', 'Back returns without saving');
    assert.match(text(await data(target,isPart)), /count:\\?\s*6\b/);
    await edit(2); await fixture.raw('type 6'); await fixture.raw('ui-key Enter');
    await fixture.command(`data get block ${target} ae2_lab_config`);
    assert.ok(String(slot(await fixture.raw('screen'),308)?.components ?? '').includes('Set stock'), 'component stack limit rejects six');
    await fixture.raw('click-slot 309');
    await until(() => fixture.raw('screen'), view => slot(view,311)?.item === 'minecraft:gold_ingot' && slot(view,311)?.count === 3, 'component-limited amount remains unchanged');
    await edit(2); await fixture.raw('type 5'); await fixture.raw('ui-key Enter');
    await until(() => fixture.raw('screen'), view => slot(view,311)?.item === 'minecraft:gold_ingot' && slot(view,311)?.count === 5, 'component stack limit accepts five');
    await fixture.raw('close');
  }
  fixture.mark('block and cable-part fake slots add and subtract held amounts; editors commit expressions and fluid units while respecting per-item component limits');
  await fixture.restart();
  for (const [target,isPart] of [[block,false],[part,true]] as const) { const saved=text(await data(target,isPart)); assert.match(saved,/count:\\?\s*6\b/); assert.match(saved,/1500L/); assert.match(saved,/minecraft:max_stack_size/); assert.match(saved,/count:\\?\s*5\b/); }
  fixture.mark('both stock targets persist across restart');

  await open(block,0); await edit(0); await fixture.raw('type 9');
  await fixture.command(`setblock ${block} minecraft:air`); await fixture.command(`setblock ${block} ae2:interface`);
  try { await fixture.raw('ui-key Enter'); } catch {}
  const replacement=text(await data(block,false)); assert.ok(!replacement.includes('minecraft:diamond'), 'stale editor cannot write to a replacement Interface');
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('a replaced physical host rejects the stale editor'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
