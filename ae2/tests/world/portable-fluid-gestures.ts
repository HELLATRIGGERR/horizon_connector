/** Portable fluid menus use FILL_ITEM on left and EMPTY_ITEM on right. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode=process.argv.includes('--in-frame')?'in-frame':'server-thread';
const fixture=new WorldFixture('portable-fluid-gestures',mode);
const water=(view:any)=>view.slots?.find((slot:any)=>slot.slot>=300&&slot.slot<345&&slot.item==='minecraft:water_bucket');
const amount=(view:any,n:number)=>String(water(view)?.components??'').includes(`${n} mB`);
try {
  await fixture.start(); await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0','minecraft:bucket');
  await fixture.replace('hotbar.1','ae2:portable_fluid_cell_1k[ae2:stored_energy=10000.0d,ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":2000L}]]');
  await fixture.select(1,'ae2:portable_fluid_cell_1k'); await fixture.raw('look 0 -80'); await fixture.raw('use-item');
  let view=await until(()=>fixture.raw('screen'),state=>water(state)&&amount(state,2000),'portable cell with 2000 mB',30000);
  await fixture.raw('click-slot 0'); await until(()=>fixture.raw('inventory'),state=>state.carried?.item==='minecraft:bucket','empty bucket held');
  await fixture.raw(`click-slot ${water(view).slot} right`); await fixture.command('data get entity @s SelectedItem');
  assert.equal((await fixture.raw('inventory')).carried?.item,'minecraft:bucket','right click does not fill');
  view=await fixture.raw('screen'); assert(amount(view,2000));
  await fixture.raw(`click-slot ${water(view).slot}`);
  await until(()=>fixture.raw('inventory'),state=>state.carried?.item==='minecraft:water_bucket','left click fills bucket');
  view=await until(()=>fixture.raw('screen'),state=>amount(state,1000),'1000 mB remains');
  await fixture.raw(`click-slot ${water(view).slot}`); await fixture.command('data get entity @s SelectedItem');
  assert.equal((await fixture.raw('inventory')).carried?.item,'minecraft:water_bucket','left click does not empty');
  view=await fixture.raw('screen'); assert(amount(view,1000));
  await fixture.raw(`click-slot ${water(view).slot} right`);
  await until(()=>fixture.raw('inventory'),state=>state.carried?.item==='minecraft:bucket','right click empties bucket');
  await until(()=>fixture.raw('screen'),state=>amount(state,2000),'2000 mB restored');
  await fixture.raw('click-slot 0'); await until(()=>fixture.raw('inventory'),state=>!state.carried&&state.hotbar?.[0]?.item==='minecraft:bucket','bucket returned');
  await fixture.replace('hotbar.1','ae2:portable_fluid_cell_1k[ae2:stored_energy=10000.0d,ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":2000L},{"#t":"ae2:i",id:"minecraft:bucket","#":2L}]]');
  await until(()=>fixture.raw('screen'),state=>amount(state,2000),'same-type owner replacement restores source mixed content');
  await fixture.replace('hotbar.0','minecraft:air'); view=await fixture.raw('screen');
  await fixture.raw(`click-slot ${water(view).slot}`);
  await until(()=>fixture.raw('inventory'),state=>state.carried?.item==='minecraft:water_bucket','hidden stored empty bucket is filled');
  view=await until(()=>fixture.raw('screen'),state=>amount(state,1000),'borrowed bucket uses 1000 mB');
  await fixture.raw(`click-slot ${water(view).slot} right`);
  await until(()=>fixture.raw('inventory'),state=>state.carried?.item==='minecraft:bucket','borrowed bucket emptied');
  await fixture.raw('click-slot 0'); await until(()=>fixture.raw('inventory'),state=>!state.carried&&state.hotbar?.[0]?.item==='minecraft:bucket','borrowed bucket returned');
  view=await until(()=>fixture.raw('screen'),state=>amount(state,2000),'fluid restored before shift-click');
  await fixture.raw(`click-slot ${water(view).slot} shift`);
  await until(()=>fixture.raw('inventory'),state=>!state.carried&&state.counts?.['minecraft:water_bucket']===1,'shift-click fills the other stored bucket into player inventory');
  await until(()=>fixture.raw('screen'),state=>amount(state,1000),'1000 mB remains after second borrowed bucket');
  await fixture.raw('close'); await fixture.restart();
  await fixture.select(1,'ae2:portable_fluid_cell_1k'); await fixture.raw('look 0 -80'); await fixture.raw('use-item');
  await until(()=>fixture.raw('screen'),state=>amount(state,1000),'portable contents survive restart');
  const inventory=await fixture.raw('inventory'); assert.equal(inventory.counts?.['minecraft:bucket'],1); assert.equal(inventory.counts?.['minecraft:water_bucket'],1);
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('portable click directions, storage-borrowed buckets and physical custody survive restart'); fixture.report('passed');
} catch(error) { fixture.report('failed',error); throw error; }
finally { await fixture.close(); }
