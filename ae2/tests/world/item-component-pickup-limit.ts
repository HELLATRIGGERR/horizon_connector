/** Physical item pickup preserves the stack's maximum component in player slots. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode:Mode=process.argv.includes('--in-frame')?'in-frame':'server-thread';const fixture=new WorldFixture('item-component-pickup-limit',mode);
try {
 const state=await fixture.start(),[px,py,pz]=state.player.position;const x=Math.floor(px)+32,y=Math.ceil(py)+5,z=Math.floor(pz)+32;
 const at=(dx:number,dy=0,dz=0)=>`${x+dx} ${y+dy} ${z+dz}`;
 await fixture.command(`tp @s ${x+.5} ${y} ${z+.5}`);
 for(const dx of [-2,10]) for(const dz of [-2,2]) await until(async()=>(await fixture.command(`execute if loaded ${at(dx,0,dz)}`)).chat??[],lines=>lines.includes('Test passed'),'loaded pickup corner',30000);
 await fixture.command(`fill ${at(-2,0,-2)} ${at(10,3,2)} minecraft:air`);await fixture.command(`fill ${at(-2,-1,-2)} ${at(10,-1,2)} minecraft:stone`);
 await fixture.command('clear @s');await fixture.command(`tp @s ${x+9.5} ${y} ${z+.5}`);
 const item='{Item:{id:"minecraft:stone",count:16,components:{"minecraft:max_stack_size":16,"minecraft:custom_data":{variant:"pickup-max"}}},PickupDelay:0s}';
 for(let i=0;i<3;i++) await fixture.command(`summon minecraft:item ${x+2.5} ${y+.2} ${z+.5} ${item}`);
 const nearby=async()=>(await fixture.raw('entities 16 minecraft:item')).entities.filter((e:any)=>e.item?.components?.includes('pickup-max'));
 await until(nearby,entities=>entities.length===3&&entities.every((e:any)=>e.item.count===16),'three separate max-16 physical item entities');
 await fixture.command(`tp @s ${x+2.5} ${y} ${z+.5}`);
 const slots=(view:any)=>[...(view.hotbar??[]),...(view.main??[])].filter((s:any)=>s.item==='minecraft:stone'&&s.components?.includes('pickup-max'));
 const inventory=await until(()=>fixture.raw('inventory'),view=>slots(view).reduce((sum:number,s:any)=>sum+s.count,0)===48,'all three item entities picked up',30000);
 assert.equal(slots(inventory).length,3);assert(slots(inventory).every((s:any)=>s.count===16));
 assert(slots(inventory).every((s:any)=>s.components.includes('minecraft:max_stack_size')),'client receives the source maximum component');
 fixture.mark('pickup preserves three physical stacks at the exact component maximum of 16');
 await fixture.restart();const restored=await fixture.raw('inventory');assert.equal(slots(restored).length,3);assert(slots(restored).every((s:any)=>s.count===16 && s.components.includes('minecraft:max_stack_size')));
 fixture.assertHidden(await fixture.raw('status'));fixture.mark('all three component-limited player stacks survive restart');fixture.report('passed');
}catch(error){fixture.report('failed',error);throw error;}finally{await fixture.close();}
