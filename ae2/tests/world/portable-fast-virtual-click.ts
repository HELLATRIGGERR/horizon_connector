/** Ordered player pickup and virtual insertion must not lose the second action. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode=process.argv.includes('--in-frame')?'in-frame':'server-thread';
const fixture=new WorldFixture('portable-fast-virtual-click',mode);
const entry=(view:any,id:string)=>view.slots?.find((slot:any)=>slot.slot>=300&&slot.slot<345&&slot.item===id);
try {
  await fixture.start(); await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0','minecraft:diamond',2);
  await fixture.replace('hotbar.1','ae2:portable_item_cell_1k[ae2:stored_energy=10000.0d,ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:gold_ingot","#":1L}]]');
  await fixture.select(1,'ae2:portable_item_cell_1k'); await fixture.raw('look 0 -80'); await fixture.raw('use-item');
  await until(()=>fixture.raw('screen'),view=>entry(view,'minecraft:gold_ingot'),'portable cell open',30000);
  // Hold the authoritative server until both real packets are queued. The
  // source applies them in order; an unrelated state update must not drop #2.
  await fixture.hold();
  try {await fixture.raw('click-slot 0'); await fixture.raw('click-slot 310');}
  finally {await fixture.hold(false);}
  await until(async()=>JSON.stringify(await fixture.command('data get entity @s SelectedItem')),data=>data.includes('minecraft:diamond'),'authoritative cell contains the immediately inserted diamonds');
  let view=await until(()=>fixture.raw('screen'),state=>entry(state,'minecraft:diamond')&&state.carried===null,'immediate player pickup followed by virtual insertion');
  assert.equal(entry(view,'minecraft:diamond').count,2);
  await fixture.raw(`click-slot ${entry(view,'minecraft:diamond').slot}`);
  await until(async()=>JSON.stringify(await fixture.command('data get entity @s SelectedItem')),data=>!data.includes('minecraft:diamond'),'authoritative cell no longer contains the recovered diamonds');
  await until(()=>fixture.raw('inventory'),state=>state.carried?.item==='minecraft:diamond'&&state.carried.count===2,'same physical diamonds recovered');
  await fixture.raw('close'); await until(()=>fixture.raw('inventory'),state=>!state.carried&&state.counts?.['minecraft:diamond']===2,'cursor returned once');
  await fixture.restart(); assert.equal((await fixture.raw('inventory')).counts?.['minecraft:diamond'],2);
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('rapid portable virtual insertion and extraction preserve the real stack across restart'); fixture.report('passed');
} catch(error) { fixture.report('failed',error); throw error; }
finally {await fixture.close();}
