/** In-frame Storage and Energy Level Emitter menus bind the original face. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';
const fixture=new WorldFixture('inframe-emitter-menu','in-frame');
const slot=(view:any,index:number)=>view.slots?.find((s:any)=>s.slot===index);
const chat=(reply:any)=>(reply.chat??[]).join('\n');
try {
  const start=await fixture.start();const [px,py,pz]=start.player.position;
  const x=Math.floor(px)+24,y=Math.ceil(py)+5,z=Math.floor(pz)+24;
  const at=(dx:number)=>`${x+dx} ${y} ${z}`,storage=at(0),energy=at(3),donor=at(6);
  await fixture.command(`tp @s ${x+3.5} ${y} ${z+2.5}`);
  for(const [dx,dz] of [[-2,-3],[-2,3],[8,-3],[8,3]] as const)
    await until(()=>fixture.raw(`block ${x+dx} ${y} ${z+dz}`),v=>v.loaded,'emitter fixture chunk loaded',30000);
  await fixture.command(`fill ${x-2} ${y} ${z-3} ${x+8} ${y+4} ${z+3} minecraft:air`);
  await fixture.command(`fill ${x-2} ${y-1} ${z-3} ${x+8} ${y-1} ${z+3} minecraft:stone`);
  for(const target of [storage,energy,donor]) await fixture.command(`setblock ${target} ae2:cable_bus`);
  await fixture.command('clear @s');await fixture.replace('hotbar.0','ae2:level_emitter');await fixture.select(0,'ae2:level_emitter');
  assert.equal((await fixture.ae(`grid part ${storage} south`)).attached,true);
  await fixture.replace('hotbar.0','ae2:energy_level_emitter');await fixture.select(0,'ae2:energy_level_emitter');
  assert.equal((await fixture.ae(`grid part ${energy} south`)).attached,true);
  assert.equal((await fixture.ae(`grid part ${donor} south`)).attached,true);
  await fixture.command('gamemode survival @s');await fixture.replace('hotbar.0','minecraft:diamond',3);
  await fixture.replace('hotbar.1','ae2:fuzzy_card');await fixture.replace('hotbar.8','minecraft:stick');
  const open=async(target:string,dx:number,title:RegExp)=>{await fixture.select(8,'minecraft:stick');await fixture.command(`tp @s ${x+dx+.5} ${y} ${z+2.5}`);await fixture.raw(`interact ${target} south`);
    return until(()=>fixture.raw('screen'),v=>title.test(JSON.stringify(v.title??'')) && slot(v,300),'local emitter menu');};
  await open(storage,0,/Level Emitter/i);await fixture.raw('click-slot 0');await fixture.raw('click-slot 300 shift');
  await until(()=>fixture.raw('screen'),v=>slot(v,300)?.item==='minecraft:diamond' && v.carried?.count===3,'storage emitter filter retains template');
  await fixture.raw('click-slot 0');await fixture.raw('click-slot 1');await fixture.raw('click-slot 336');
  await until(()=>fixture.raw('screen'),v=>slot(v,336)?.item==='ae2:fuzzy_card' && !v.carried,'physical fuzzy card inserted');
  await fixture.raw('click-slot 345');await fixture.raw('close');
  await open(energy,3,/Energy Level Emitter/i);await fixture.raw('type =2*3');await fixture.raw('ui-key Enter');
  await until(async()=>chat(await fixture.command(`data get block ${energy} ae2_lab_config.parts`)),v=>/emitter_threshold:\s*6L/.test(v),'energy expression updates the owned part');
  await fixture.raw('click-slot 342');await fixture.raw('close');await fixture.restart();
  const stored=chat(await fixture.command(`data get block ${storage} ae2_lab_config.parts`));assert(stored.includes('minecraft:diamond'));assert(stored.includes('ae2:fuzzy_card'));
  const powered=chat(await fixture.command(`data get block ${energy} ae2_lab_config.parts`));assert.match(powered,/emitter_threshold:\s*6L/);
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:diamond'],3);fixture.mark('emitter filter, physical card and energy threshold survive restart');
  await open(energy,3,/Energy Level Emitter/i);await fixture.raw('click-slot 0');
  await until(()=>fixture.raw('inventory'),v=>v.carried?.item==='minecraft:diamond' && v.carried.count===3,'real cursor before energy emitter replacement');
  await fixture.command(`data modify block ${energy} ae2_lab_config.parts[{face:"south"}] set from block ${donor} ae2_lab_config.parts[{face:"south"}]`);
  await until(()=>fixture.raw('screen'),v=>v.screen==='playing','replaced energy face closes old menu');
  const inv=await fixture.raw('inventory');assert.equal(inv.counts?.['minecraft:diamond'],3);assert(!inv.carried);
  fixture.assertHidden(await fixture.raw('status'));fixture.mark('replacement retains exact player cursor custody');fixture.report('passed');
} catch(error){fixture.report('failed',error);throw error;}
finally{await fixture.close();}
