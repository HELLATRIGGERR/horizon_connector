/** A Pattern Access menu belongs to the clicked part instance, not its face alone. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode:Mode=process.argv.includes('--in-frame')?'in-frame':'server-thread';
const fixture=new WorldFixture('pattern-access-owner',mode);
const slot=(view:any,index:number)=>view.slots?.find((s:any)=>s.slot===index);
const chat=(reply:any)=>(reply.chat??[]).join('\n');
try {
  const start=await fixture.start();const [px,py,pz]=start.player.position;
  const x=Math.floor(px)+24,y=Math.ceil(py)+5,z=Math.floor(pz)+24;
  const at=(dx:number)=>`${x+dx} ${y} ${z}`,access=at(2),donor=at(5);
  await fixture.command(`tp @s ${x+2.5} ${y} ${z+2.5}`);
  for(const [dx,dz] of [[-2,-3],[-2,3],[7,-3],[7,3]] as const)
    await until(()=>fixture.raw(`block ${x+dx} ${y} ${z+dz}`),v=>v.loaded,'Pattern Access fixture chunks loaded',30000);
  await fixture.command(`fill ${x-2} ${y} ${z-3} ${x+7} ${y+4} ${z+3} minecraft:air`);
  await fixture.command(`fill ${x-2} ${y-1} ${z-3} ${x+7} ${y-1} ${z+3} minecraft:stone`);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`);await fixture.command(`setblock ${at(1)} ae2:drive`);
  await fixture.command(`setblock ${access} ae2:cable_bus`);await fixture.command(`setblock ${donor} ae2:cable_bus`);
  await fixture.command('clear @s');await fixture.replace('hotbar.0','ae2:pattern_access_terminal');await fixture.select(0,'ae2:pattern_access_terminal');
  assert.equal((await fixture.ae(`grid part ${access} south`)).attached,true);
  assert.equal((await fixture.ae(`grid part ${donor} south`)).attached,true);
  await until(()=>fixture.ae(`grid status ${access}`),v=>v.online && v.powered,'powered Pattern Access host',30000);
  await fixture.command('gamemode survival @s');await fixture.replace('hotbar.0','minecraft:diamond',3);
  await fixture.replace('hotbar.8','minecraft:stick');await fixture.select(8,'minecraft:stick');
  await fixture.command(`tp @s ${x+2.5} ${y} ${z+2.5}`);await fixture.raw(`interact ${access} south`);
  await until(()=>fixture.raw('screen'),v=>/Pattern Access/i.test(JSON.stringify(v.title??'')) && slot(v,0),'physical Pattern Access menu');
  await fixture.raw('click-slot 0');await until(()=>fixture.raw('inventory'),v=>v.carried?.item==='minecraft:diamond' && v.carried.count===3,'physical cursor before replacement');
  await fixture.command(`data modify block ${access} ae2_lab_config.parts[{face:"south"}] set from block ${donor} ae2_lab_config.parts[{face:"south"}]`);
  await until(()=>fixture.raw('screen'),v=>v.screen==='playing','same-kind terminal replacement closes old menu');
  const inventory=await fixture.raw('inventory');assert.equal(inventory.counts?.['minecraft:diamond'],3);assert(!inventory.carried);
  const saved=chat(await fixture.command(`data get block ${access} ae2_lab_config.parts`));assert(!saved.includes('ae2_access_filter'));
  fixture.assertHidden(await fixture.raw('status'));fixture.mark('same-kind face replacement returns the exact cursor and cannot inherit an old session');fixture.report('passed');
} catch(error){fixture.report('failed',error);throw error;}
finally{await fixture.close();}
