/** Storage Level Emitter uses ordinary source FakeSlot filter actions. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture=new WorldFixture('emitter-fake-gestures',mode);
const slot=(view:any,index:number)=>view.slots?.find((s:any)=>s.slot===index);
const chat=(reply:any)=>(reply.chat??[]).join('\n');
try {
  const start=await fixture.start();const [px,py,pz]=start.player.position;
  const x=Math.floor(px)+24,y=Math.ceil(py)+5,z=Math.floor(pz)+24,host=`${x} ${y} ${z}`;
  await fixture.command(`tp @s ${x+.5} ${y} ${z+2.5}`);
  for(const [dx,dz] of [[-2,-3],[-2,3],[2,-3],[2,3]] as const)
    await until(()=>fixture.raw(`block ${x+dx} ${y} ${z+dz}`),v=>v.loaded,'emitter fixture chunk loaded',30000);
  await fixture.command(`fill ${x-2} ${y} ${z-3} ${x+2} ${y+4} ${z+3} minecraft:air`);
  await fixture.command(`fill ${x-2} ${y-1} ${z-3} ${x+2} ${y-1} ${z+3} minecraft:stone`);
  await fixture.command(`setblock ${host} ae2:cable_bus`);await fixture.command('clear @s');
  await fixture.replace('hotbar.0','ae2:level_emitter');await fixture.select(0,'ae2:level_emitter');
  assert.equal((await fixture.ae(`grid part ${host} south`)).attached,true);
  await fixture.command('gamemode survival @s');await fixture.replace('hotbar.0','minecraft:diamond',2);
  await fixture.replace('hotbar.1','minecraft:water_bucket');await fixture.replace('hotbar.8','minecraft:stick');await fixture.select(8,'minecraft:stick');
  await fixture.command(`tp @s ${x+.5} ${y} ${z+2.5}`);await fixture.raw(`interact ${host} south`);
  await until(()=>fixture.raw('screen'),v=>/Level Emitter/i.test(JSON.stringify(v.title??'')) && slot(v,300),'Storage Level Emitter menu');
  await fixture.raw('click-slot 0');await fixture.raw('click-slot 300 shift');
  await until(async()=>chat(await fixture.command(`data get block ${host} ae2_lab_config.parts`)),v=>v.includes('emitter_filter') && v.includes('minecraft:diamond'),'shift-click writes source item filter');
  await until(()=>fixture.raw('screen'),v=>slot(v,300)?.item==='minecraft:diamond' && v.carried?.item==='minecraft:diamond' && v.carried.count===2,'shift filter preserves the real diamonds');
  await fixture.raw('click-slot 0');await fixture.raw('click-slot 1');await fixture.raw('click-slot 300');
  await until(async()=>chat(await fixture.command(`data get block ${host} ae2_lab_config.parts`)),v=>v.includes('emitter_filter') && v.includes('minecraft:water_bucket') && !v.includes('emitter_fluid_filter'),'left-click selects bucket item');
  await fixture.raw('click-slot 300 right');
  await until(async()=>chat(await fixture.command(`data get block ${host} ae2_lab_config.parts`)),v=>v.includes('emitter_fluid_filter') && v.includes('minecraft:water') && !v.includes('emitter_filter:'),'right-click empties the bucket as a fluid filter');
  await until(()=>fixture.raw('screen'),v=>String(slot(v,300)?.components??'').includes('Fluid filter: minecraft:water') && v.carried?.item==='minecraft:water_bucket','fluid filter preserves the real bucket');
  await fixture.raw('click-slot 1');await fixture.raw('close');await fixture.restart();
  const saved=chat(await fixture.command(`data get block ${host} ae2_lab_config.parts`));assert(saved.includes('emitter_fluid_filter'));assert(saved.includes('minecraft:water'));
  const inv=await fixture.raw('inventory');assert.equal(inv.counts?.['minecraft:diamond'],2);assert.equal(inv.counts?.['minecraft:water_bucket'],1);
  fixture.assertHidden(await fixture.raw('status'));fixture.mark('source emitter fake-slot actions preserve physical item and bucket custody through restart');fixture.report('passed');
} catch(error){fixture.report('failed',error);throw error;}
finally{await fixture.close();}
