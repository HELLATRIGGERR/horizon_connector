/** A remote Formation Plane must use its own loaded ServerLevel across a Quantum bridge. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';
const fixture=new WorldFixture('formation-quantum-dimensions','server-thread');
const dimensions=['minecraft:overworld','minecraft:the_nether'] as const;
const x=40,y=100,z=40;
const at=(dx=0,dy=0,dz=0)=>`${x+dx} ${y+dy} ${z+dz}`;
const command=(dim:string,text:string)=>fixture.command(`execute in ${dim} run ${text}`);
const ae=async(dim:string,text:string)=>{
 const status=await fixture.raw("status");
 assert.equal(status.dimension,dim,"AE2 diagnostic uses the actual active player dimension");
 const reply=await fixture.command('ae2 '+text);
 const line=reply.chat?.findLast((line:string)=>line.startsWith('AE2 {'));
 assert(line,JSON.stringify(reply));return JSON.parse(line.slice(4));
};
const remoteBlock=async()=>{
 const reply=await command(dimensions[1],`execute if block ${at(3)} ae2:energy_cell`);
 return reply.chat?.includes('Test passed')??false;
};
try {
 await fixture.start();
 for(const dim of dimensions) {
  await command(dim,`tp @s ${x+.5} ${y+4} ${z+.5}`);
  await command(dim,`forceload add ${x-8} ${z-8} ${x+8} ${z+8}`);
  for(const [dx,dz] of [[-4,-4],[6,4]]) await until(async()=>{
   const reply=await command(dim,`execute if loaded ${at(dx,0,dz)}`);return reply.chat?.includes('Test passed');
  },Boolean,`${dim} physical frame corners loaded`,90000);
  await command(dim,`fill ${at(-4,-3,-4)} ${at(6,3,4)} minecraft:air`);
  await command(dim,`fill ${at(-4,-3,-4)} ${at(6,-3,4)} minecraft:stone`);
  for(let dx=-1;dx<=1;dx++)for(let dz=-1;dz<=1;dz++) {
   await command(dim,`setblock ${at(dx,0,dz)} ae2:${dx===0&&dz===0?'quantum_link':'quantum_ring'}`);
  }
  await command(dim,`item replace block ${at()} container.0 with ae2:quantum_entangled_singularity[ae2:entangled_singularity_id=731]`);
 }
 await command(dimensions[0],`setblock ${at(-2)} ae2:creative_energy_cell`);
 await command(dimensions[1],`setblock ${at(2)} ae2:cable_bus`);
 await fixture.command('clear @s');
 await fixture.replace('hotbar.0','ae2:formation_plane');await fixture.select(0,'ae2:formation_plane');
 assert.equal((await ae(dimensions[1],`grid part ${at(2)} east`)).attached,true);
 await until(()=>ae(dimensions[1],`grid part-status ${at(2)} east`),state=>state.online&&state.powered,'remote plane powered through the Quantum bridge',60000);
 await command(dimensions[0],`tp @s ${x+.5} ${y+2} ${z-3.5}`);
 const charged='ae2:energy_cell[ae2:stored_energy=1234.0d,minecraft:custom_name={text:"Remote Formation Battery",color:"red"}]';
 await fixture.replace('hotbar.1',charged);await fixture.select(1,'ae2:energy_cell');
 assert.equal((await fixture.ae(`grid deposit ${at(-2)}`)).inserted,1,'the physical charged cell reaches the remote plane');
 assert(await remoteBlock(),'the target block is placed in the Nether');
 const remote=JSON.stringify(await command(dimensions[1],`data get block ${at(3)}`));
 assert.match(remote,/Remote Formation Battery/);assert.match(remote,/color.{0,12}red/);
 const local=await fixture.raw(`getblock ${at(3)}`);assert.match(local.block,/^minecraft:air(?:\[|$)/,'the Overworld coordinate stays empty');
 assert.doesNotMatch(JSON.stringify(await fixture.command('data get entity @s Inventory')),/ae2:energy_cell/);
 await command(dimensions[1],`tp @s ${x+.5} ${y+2} ${z-3.5}`);
 assert.equal((await ae(dimensions[1],`power ${at(3)} east`)).energy,1234);
 fixture.mark('Quantum insertion consumes one real component-bearing item and places it only in the remote dimension');
 await fixture.restart();
 await until(()=>ae(dimensions[1],`grid part-status ${at(2)} east`),state=>state.online&&state.powered,'remote plane restored',60000);
 assert(await remoteBlock());assert.match(JSON.stringify(await command(dimensions[1],`data get block ${at(3)}`)),/Remote Formation Battery/);
 // Remove the previously verified fixture block before offering fluid.
 await command(dimensions[1],`setblock ${at(3)} minecraft:air`);
 await command(dimensions[0],`tp @s ${x+.5} ${y+2} ${z-3.5}`);
 await fixture.replace('hotbar.1','minecraft:water_bucket');await fixture.select(1,'minecraft:water_bucket');
 assert.equal((await fixture.ae(`fluid grid insert ${at(-2)}`)).inserted_mb,1000);
 assert.match(JSON.stringify(await fixture.command('data get entity @s SelectedItem')),/minecraft:bucket/);
 assert(!(await remoteBlock()));
 const empty=await command(dimensions[1],`execute if block ${at(3)} minecraft:air`);
 assert(empty.chat?.includes('Test passed'),'remote water evaporates according to the Nether rules');
 fixture.assertHidden(await fixture.raw('status'));
 fixture.mark('remote component custody survives restart and remote fluid placement uses Nether rules');fixture.report('passed');
}catch(error){fixture.report('failed',error);throw error;}finally{await fixture.close();}
