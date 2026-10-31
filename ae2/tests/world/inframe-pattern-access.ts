/** In-frame Pattern Access reads and changes real Provider slots. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';
const fixture=new WorldFixture('inframe-pattern-access','in-frame');
const slot=(view:any,index:number)=>view.slots?.find((s:any)=>s.slot===index);
const chat=(reply:any)=>(reply.chat??[]).join('\n');
try {
  const start=await fixture.start();const [px,py,pz]=start.player.position;
  const x=Math.floor(px)+24,y=Math.ceil(py)+5,z=Math.floor(pz)+24;
  const at=(dx:number)=>`${x+dx} ${y} ${z}`,access=at(2),provider=at(3);
  await fixture.command(`tp @s ${x+2.5} ${y} ${z+2.5}`);
  for(const [dx,dz] of [[-2,-3],[-2,3],[5,-3],[5,3]] as const)
    await until(()=>fixture.raw(`block ${x+dx} ${y} ${z+dz}`),v=>v.loaded,'Pattern Access fixture chunks loaded',30000);
  await fixture.command(`fill ${x-2} ${y} ${z-3} ${x+5} ${y+4} ${z+3} minecraft:air`);
  await fixture.command(`fill ${x-2} ${y-1} ${z-3} ${x+5} ${y-1} ${z+3} minecraft:stone`);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`);await fixture.command(`setblock ${at(1)} ae2:drive`);
  await fixture.command(`setblock ${access} ae2:cable_bus`);await fixture.command(`setblock ${provider} ae2:pattern_provider`);
  await fixture.command('clear @s');await fixture.replace('hotbar.0','ae2:pattern_access_terminal');await fixture.select(0,'ae2:pattern_access_terminal');
  assert.equal((await fixture.ae(`grid part ${access} south`)).attached,true);
  await until(()=>fixture.ae(`grid status ${access}`),v=>v.online && v.powered,'powered Pattern Access host',30000);
  await fixture.replace('hotbar.0','ae2:blank_pattern');await fixture.select(0,'ae2:blank_pattern');
  assert.equal((await fixture.ae('crafting encode minecraft:oak_log - - - - - - - -')).encoded,true);
  await fixture.command('gamemode survival @s');await fixture.replace('hotbar.8','minecraft:stick');await fixture.select(8,'minecraft:stick');
  await fixture.command(`tp @s ${x+3.5} ${y} ${z+2.5}`);await fixture.raw(`interact ${provider} south`);
  let view=await until(()=>fixture.raw('screen'),v=>/Pattern Provider/i.test(JSON.stringify(v.title??'')) && slot(v,300),'physical Provider menu');
  const pattern=view.slots.find((s:any)=>s.slot<36 && s.item==='ae2:crafting_pattern');assert(pattern);
  await fixture.raw(`click-slot ${pattern.slot} shift`);await until(()=>fixture.raw('screen'),v=>slot(v,300)?.item==='ae2:crafting_pattern','physical pattern enters Provider');
  await fixture.raw('close');await fixture.command(`tp @s ${x+2.5} ${y} ${z+2.5}`);await fixture.raw(`interact ${access} south`);
  view=await until(()=>fixture.raw('screen'),v=>/Pattern Access/i.test(JSON.stringify(v.title??'')) && v.pattern_layout?.rows?.some((r:any)=>r.kind==='provider'),'local Pattern Access with live Provider');
  const row=view.pattern_layout.rows.findIndex((r:any)=>r.kind==='provider');const target=300+row*9;assert.equal(slot(view,target)?.item,'ae2:crafting_pattern');
  await fixture.raw(`click-slot ${target}`);await until(()=>fixture.raw('inventory'),v=>v.carried?.item==='ae2:crafting_pattern','real encoded pattern picked up from Provider');
  await fixture.raw('click-slot 0');await until(()=>fixture.raw('inventory'),v=>!v.carried && v.counts?.['ae2:crafting_pattern']===1,'real encoded pattern returned to player');
  await fixture.raw('click-slot 0 shift');await until(()=>fixture.raw('screen'),v=>slot(v,target)?.item==='ae2:crafting_pattern','shift moves real pattern back to Provider');
  assert.equal((await fixture.raw('inventory')).counts?.['ae2:crafting_pattern']??0,0);await fixture.raw('close');await fixture.restart();
  assert.match(chat(await fixture.command(`data get block ${provider}`)),/ae2:crafting_pattern/);
  assert.equal((await fixture.raw('inventory')).counts?.['ae2:crafting_pattern']??0,0);
  fixture.assertHidden(await fixture.raw('status'));fixture.mark('Pattern Access transfers one real encoded pattern in both directions and preserves it across restart');fixture.report('passed');
} catch(error){fixture.report('failed',error);throw error;}
finally{await fixture.close();}
