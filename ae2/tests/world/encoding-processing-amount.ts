/** Source middle-click SetProcessingPatternAmountScreen for item and fluid ghosts. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('encoding-processing-amount', 'server-thread');
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
const text = (reply: any) => JSON.stringify(reply.chat ?? []);
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px)+32, y = Math.ceil(py)+5, z = Math.floor(pz)+32;
  const at = (dx: number) => `${x+dx} ${y} ${z}`, terminal=at(2);
  await fixture.command(`fill ${x-2} ${y} ${z-3} ${x+5} ${y+4} ${z+3} minecraft:air`);
  await fixture.command(`fill ${x-2} ${y-1} ${z-3} ${x+5} ${y-1} ${z+3} minecraft:stone`);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`); await fixture.command(`setblock ${at(1)} ae2:drive`); await fixture.command(`setblock ${terminal} ae2:cable_bus`); await fixture.command('clear @s');
  await fixture.replace('hotbar.0','ae2:pattern_encoding_terminal'); await fixture.select(0,'ae2:pattern_encoding_terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached,true);
  await until(() => fixture.ae(`grid status ${terminal}`), state => state.online && state.powered, 'powered encoding face',30000);
  await fixture.command('gamemode survival @s'); await fixture.replace('hotbar.0','minecraft:oak_planks',3); await fixture.replace('hotbar.1','minecraft:water_bucket'); await fixture.replace('hotbar.8','minecraft:stick'); await fixture.select(8,'minecraft:stick');
  await fixture.command(`tp @s ${x+2.5} ${y} ${z-2.5}`); await fixture.raw(`interact ${terminal} north`);
  await until(() => fixture.raw('screen'), view => String(slot(view,348)?.components ?? '').includes('Encode crafting pattern'), 'encoding terminal');
  await fixture.raw('click-slot 348');
  await until(() => fixture.raw('screen'), view => String(slot(view,348)?.components ?? '').includes('Back to storage'), 'encoding editor');
  await fixture.raw('click-slot 346');
  await until(() => fixture.raw('screen'), view => String(slot(view,346)?.components ?? '').includes('processing'), 'processing mode');
  await fixture.raw('click-slot 0'); await until(() => fixture.raw('inventory'), view => view.carried?.item==='minecraft:oak_planks','item template on cursor');
  await fixture.raw('click-slot 300'); await fixture.command(`data get block ${terminal} ae2_lab_config.parts`);
  await until(() => fixture.raw('screen'), view => slot(view,300)?.item==='minecraft:oak_planks' && slot(view,300)?.count===3, 'item ghost');
  await fixture.raw('click-slot 0'); await until(() => fixture.raw('inventory'), view => !view.carried,'item template returned');
  await fixture.raw('click-slot 1'); await until(() => fixture.raw('inventory'), view => view.carried?.item==='minecraft:water_bucket','filled bucket on cursor');
  await fixture.raw('click-slot 318 right'); await fixture.command(`data get block ${terminal} ae2_lab_config.parts`);
  await until(() => fixture.raw('screen'), view => slot(view,318)?.item==='ae2:wrapped_generic_stack' && String(slot(view,318)?.components ?? '').includes('minecraft:water'), 'fluid output ghost');
  const data=()=>fixture.command(`data get block ${terminal} ae2_lab_config.parts`);
  const editor=async(index:number) => {
    await fixture.raw(`click slot-${index} middle`);
    await until(() => fixture.raw('screen'), view => String(slot(view,300)?.components ?? '').includes('Amount +1') && String(slot(view,308)?.components ?? '').includes('Set processing'), 'processing amount editor');
  };
  await editor(300); await fixture.raw('type =2*3'); assert.match(text(await data()),/count:\\?\s*3\b/,'typing stays pending');
  await fixture.raw('ui-key Enter'); await until(() => fixture.raw('screen'), view => slot(view,300)?.item==='minecraft:oak_planks' && slot(view,300)?.count===6, 'item amount committed');
  assert.equal((await fixture.raw('inventory')).carried?.item,'minecraft:water_bucket','parent cursor survives submenu');
  await editor(318); await fixture.raw('type 1.5'); await fixture.raw('ui-key Enter');
  await until(data, reply => /1500L/.test(text(reply)), 'fractional bucket amount committed');
  await until(() => fixture.raw('screen'), view => String(slot(view,318)?.components ?? '').includes('minecraft:water'), 'returned to processing grid');
  await editor(300); await fixture.raw('type 25'); await fixture.raw('click-slot 309');
  await until(() => fixture.raw('screen'), view => slot(view,300)?.count===6, 'Back cancels');
  await fixture.raw('click-slot 1'); await until(() => fixture.raw('inventory'), view => !view.carried && view.counts?.['minecraft:water_bucket']===1,'physical bucket returned');
  await fixture.raw('close'); await fixture.restart();
  const saved=text(await data()); assert.match(saved,/count:\\?\s*6\b/); assert.match(saved,/1500L/);
  const inventory=await fixture.raw('inventory'); assert.equal(inventory.counts?.['minecraft:oak_planks'],3); assert.equal(inventory.counts?.['minecraft:water_bucket'],1);
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('middle-click edits item and fluid processing amounts without consuming templates; Back cancels and restart retains both values'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
