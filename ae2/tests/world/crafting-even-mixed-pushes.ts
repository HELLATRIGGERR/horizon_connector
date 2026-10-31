/** Two source pushes preserve per-push choices even when aggregate counts divide evenly. */
import assert from 'node:assert/strict';
import { cpSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { root } from '../../scripts/common';
import { WorldFixture, until, type Mode } from './harness';
const isolated=mkdtempSync(join(tmpdir(),'ae2-even-mixed-')), content=join(isolated,'content');
cpSync(process.env.AE2_CONTENT_DIR ?? join(root,'content/ae2'),content,{recursive:true});
const recipes=join(content,'data/ae2/recipe/test');mkdirSync(recipes,{recursive:true});
writeFileSync(join(recipes,'even_mixed.json'),JSON.stringify({type:'minecraft:crafting_shapeless',ingredients:Array(3).fill(['minecraft:stone','minecraft:dirt']),result:{id:'minecraft:diamond',count:1}}));
const mode:Mode=process.argv.includes('--in-frame')?'in-frame':'server-thread';const fixture=new WorldFixture('crafting-even-mixed-pushes',mode,{content});
const pattern='ae2:crafting_pattern[ae2:encoded_crafting_pattern={inputs:[{id:"minecraft:stone",count:1},{id:"minecraft:stone",count:1},{id:"minecraft:stone",count:1},{},{},{},{},{},{}],result:{id:"minecraft:diamond",count:1},recipeId:"ae2:test/even_mixed",canSubstitute:1b,canSubstituteFluids:0b}]';
const count=(status:any,item:string)=>status.items?.find((entry:any)=>entry.item===item)?.count??0;
try {
 const state=await fixture.start(),[px,py,pz]=state.player.position;
 const x=Math.floor(px)+32,y=Math.ceil(py)+5,z=Math.floor(pz)+32;
 const at=(dx:number,dy=0,dz=0)=>`${x+dx} ${y+dy} ${z+dz}`;
 const drive=at(1),terminal=at(2),cpu=at(2,1),provider=at(3),assembler=at(3,0,-1);
 await fixture.command(`tp @s ${x+.5} ${y} ${z+.5}`);
 for(const dx of [-2,6]) for(const dz of [-3,3]) await until(async()=>(await fixture.command(`execute if loaded ${at(dx,0,dz)}`)).chat??[],lines=>lines.includes('Test passed'),'loaded build corner',30000);
 await fixture.command('clear @s');await fixture.command(`fill ${at(-2,0,-3)} ${at(6,5,3)} minecraft:air`);await fixture.command(`fill ${at(-2,-1,-3)} ${at(6,-1,3)} minecraft:stone`);
 for(const [pos,id] of [[at(0),'creative_energy_cell'],[drive,'drive'],[terminal,'cable_bus'],[cpu,'64k_crafting_storage'],[provider,'pattern_provider'],[assembler,'molecular_assembler']] as const) await fixture.command(`setblock ${pos} ae2:${id}`);
 await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
 await fixture.replace('hotbar.0','ae2:terminal');await fixture.select(0,'ae2:terminal');assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached,true);
 const ready=()=>until(()=>fixture.ae(`grid status ${drive}`),s=>s.online&&s.powered,'even mixed network online',30000);
 await ready();for(const pos of [provider,assembler]) await until(()=>fixture.ae(`grid status ${pos}`),s=>s.online&&s.powered,`online ${pos}`,30000);
 await fixture.replace('hotbar.0',pattern);await fixture.replace('hotbar.8','minecraft:blaze_rod');await fixture.select(8,'minecraft:blaze_rod');await fixture.command(`tp @s ${x+5.5} ${y} ${z+.5}`);await fixture.raw(`interact ${provider} east`);
 const menu=await until(()=>fixture.raw('screen'),s=>s.slots?.some((v:any)=>v.slot<36&&v.item==='ae2:crafting_pattern'),'even mixed physical pattern');
 await fixture.raw(`click-slot ${menu.slots.find((v:any)=>v.slot<36&&v.item==='ae2:crafting_pattern').slot} shift`);await until(()=>fixture.raw('inventory'),s=>!s.counts?.['ae2:crafting_pattern'],'pattern installed');await fixture.raw('close');
 for(const [id,n] of [['minecraft:stone',4],['minecraft:dirt',2]] as const){await fixture.replace('hotbar.0',id,n);await fixture.select(0,id);assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted,n);}
 assert.equal(count(await ready(),'minecraft:stone'),4);assert.equal(count(await ready(),'minecraft:dirt'),2);
 await fixture.select(8,'minecraft:blaze_rod');await fixture.command(`tp @s ${x+2.5} ${y} ${z-2.5}`);await fixture.raw(`interact ${terminal} north`);
 const screen=await until(()=>fixture.raw('screen'),s=>s.slots?.some((v:any)=>v.slot>=300&&v.slot<345&&v.item==='minecraft:diamond'),'diamond recipe shown');
 await fixture.raw(`click-slot ${screen.slots.find((v:any)=>v.slot>=300&&v.slot<345&&v.item==='minecraft:diamond').slot} shift`);await until(()=>fixture.raw('screen'),s=>JSON.stringify(s.title??'').includes('AE2 Craft Amount'),'amount');
 await fixture.raw('ui-key right');await fixture.raw('ui-key backspace');await fixture.raw('type 2');await fixture.raw('click-slot 302');
 const confirm=await until(()=>fixture.raw('screen'),s=>JSON.stringify(s.title??'').includes('AE2 Craft Confirm')&&/bytes|Missing materials|craft planning:/.test(JSON.stringify(s)),'source tree plan');
 assert.doesNotMatch(JSON.stringify(confirm),/Missing materials|craft planning:/);assert.match(JSON.stringify(confirm),/26 bytes/);
 await fixture.hold();await fixture.raw('click-slot 302');await fixture.raw('close');
 const slots=async()=>(await fixture.command(`data get block ${assembler} Items`)).chat?.join('\n')??'';
 let first='';for(let i=0;i<40;i++){await fixture.stepServer(1);first=await slots();if((first.match(/minecraft:stone/g)??[]).length+(first.match(/minecraft:dirt/g)??[]).length===3)break;}
 assert.equal((first.match(/minecraft:stone/g)??[]).length,3,`source first push uses three primary inputs: ${first}`);assert.doesNotMatch(first,/minecraft:dirt/);
 let second='';for(let i=0;i<40;i++){await fixture.stepServer(1);second=await slots();if(second.includes('minecraft:dirt'))break;}
 assert.equal((second.match(/minecraft:stone/g)??[]).length,1,`second push retains one primary input: ${second}`);assert.equal((second.match(/minecraft:dirt/g)??[]).length,2);
 fixture.mark('the actual assembler receives 3 primary items first, then 1 primary plus 2 substitutes');
 await fixture.hold(false);await until(ready,s=>count(s,'minecraft:diamond')===2&&count(s,'minecraft:stone')===0&&count(s,'minecraft:dirt')===0,'both physical pushes complete',90000);
 await fixture.restart();assert.equal(count(await ready(),'minecraft:diamond'),2);fixture.assertHidden(await fixture.raw('status'));fixture.mark('both outputs survive restart with no surplus ingredients');fixture.report('passed');
}catch(error){fixture.report('failed',error);throw error;}finally{await fixture.close();}
