/** A filled bucket overflow stays physical when every player slot is occupied. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode=process.argv.includes('--in-frame')?'in-frame':'server-thread';
const fixture=new WorldFixture('portable-fluid-overflow',mode);
const water=(view:any)=>view.slots?.find((slot:any)=>slot.slot>=300&&slot.slot<345&&slot.item==='minecraft:water_bucket');
try {
  await fixture.start(); await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  for(let i=0;i<27;i++) await fixture.command(`item replace entity @s inventory.${i} with minecraft:dirt 64`);
  for(let i=0;i<9;i++) await fixture.replace(`hotbar.${i}`,'minecraft:dirt',64);
  await fixture.replace('hotbar.0','minecraft:bucket',2);
  await fixture.replace('hotbar.1','ae2:portable_fluid_cell_1k[ae2:stored_energy=10000.0d,ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":2000L}]]');
  await fixture.select(1,'ae2:portable_fluid_cell_1k'); await fixture.raw('look 0 -80'); await fixture.raw('use-item');
  let view=await until(()=>fixture.raw('screen'),state=>water(state),'portable cell open with water',30000);
  await fixture.raw('click-slot 0'); await until(()=>fixture.raw('inventory'),state=>state.carried?.item==='minecraft:bucket'&&state.carried.count===2,'two empty buckets held');
  await fixture.replace('hotbar.0','minecraft:dirt',64);
  view=await until(()=>fixture.raw('screen'),state=>water(state)&&state.carried?.count===2,'full inventory and carried buckets acknowledged');
  await fixture.raw(`click-slot ${water(view).slot}`);
  await until(()=>fixture.raw('inventory'),state=>state.carried?.item==='minecraft:bucket'&&state.carried.count===1,'one empty bucket remains on cursor');
  const entities=await until(()=>fixture.raw('entities 16 minecraft:item'),state=>state.entities?.some((entity:any)=>entity.item?.item==='minecraft:water_bucket'&&entity.item.count===1),'filled bucket appears as a real item entity');
  assert.equal(entities.entities.filter((entity:any)=>entity.item?.item==='minecraft:water_bucket').reduce((n:number,entity:any)=>n+entity.item.count,0),1);
  await until(()=>fixture.raw('screen'),state=>String(water(state)?.components??'').includes('1000 mB'),'exactly one bucket of fluid was consumed');
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('full player inventory drops one physical filled bucket while retaining the remaining carried bucket'); fixture.report('passed');
} catch(error) { fixture.report('failed',error); throw error; }
finally { await fixture.close(); }
