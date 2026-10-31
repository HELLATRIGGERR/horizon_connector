/** Compare actual GUI icons in an offscreen framebuffer. */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { inflateSync } from 'node:zlib';
import { WorldFixture, until } from './harness';

// Horizon writes 8-bit, non-interlaced RGBA PNG screenshots. Decode those
// directly with Node's built-in zlib so this fixture has no image dependency.
function screenshot(path:string) {
  const png=readFileSync(path);assert.equal(png.subarray(0,8).toString('hex'),'89504e470d0a1a0a');
  let width=0,height=0;const chunks:Buffer[]=[];
  for(let p=8;p+12<=png.length;) {
    const n=png.readUInt32BE(p),type=png.toString('ascii',p+4,p+8),body=png.subarray(p+8,p+8+n);
    if(type==='IHDR') {width=body.readUInt32BE(0);height=body.readUInt32BE(4);assert.deepEqual([...body.subarray(8,13)],[8,6,0,0,0]);}
    if(type==='IDAT') chunks.push(body);p+=n+12;if(type==='IEND')break;
  }
  const source=inflateSync(Buffer.concat(chunks)),stride=width*4,pixels=Buffer.alloc(stride*height);
  assert.equal(source.length,(stride+1)*height);
  for(let y=0;y<height;y++) {
    const filter=source[y*(stride+1)],start=y*stride;
    for(let x=0;x<stride;x++) {
      const left=x>=4?pixels[start+x-4]:0,up=y?pixels[start+x-stride]:0,corner=y&&x>=4?pixels[start+x-stride-4]:0;
      const paeth=()=>{const p=left+up-corner,a=Math.abs(p-left),b=Math.abs(p-up),c=Math.abs(p-corner);return a<=b&&a<=c?left:b<=c?up:corner;};
      const predictor=filter===0?0:filter===1?left:filter===2?up:filter===3?Math.floor((left+up)/2):filter===4?paeth():NaN;
      assert(Number.isFinite(predictor),'supported PNG row filter');pixels[start+x]=(source[y*(stride+1)+1+x]+predictor)&255;
    }
  }
  return {width,height,pixels};
}

const fixture=new WorldFixture('dynamic-item-models','server-thread');
try {
  await fixture.start();await fixture.command('gamemode survival @s');
  await until(()=>fixture.raw('status'),s=>s.creative===false,'survival inventory');
  await fixture.command('clear @s');
  await fixture.replace('hotbar.0','ae2:memory_card');
  await fixture.replace('hotbar.1','ae2:memory_card[minecraft:custom_data={ae2_p2p_card:{id:"ae2:me_p2p_tunnel",frequency:4660s}}]');
  const wrap=(kind:string,id:string)=>`ae2:wrapped_generic_stack[minecraft:custom_data={"ae2:wrapped_stack":{"#t":"${kind}",id:"${id}","#":1000L}}]`;
  await fixture.replace('hotbar.2',wrap('ae2:f','minecraft:water'));
  await fixture.replace('hotbar.3',wrap('ae2:f','minecraft:lava'));
  await fixture.replace('hotbar.4','minecraft:diamond');
  await fixture.replace('hotbar.5',wrap('ae2:i','minecraft:diamond'));
  const inventory=await fixture.raw('inventory');
  assert.match(String(inventory.hotbar[1].components),/ae2_p2p_card.*frequency:4660s/);
  for(const index of [2,3,5]) assert.match(String(inventory.hotbar[index].components),/ae2:wrapped_stack/);
  await fixture.raw('open-inventory');
  const ui=await until(()=>fixture.raw('ui'),u=>u.screen==='inventory'&&u.widgets?.some((w:any)=>w.id==='slot-5'),'ordinary inventory widgets');
  const imagePath=join(fixture.run,'dynamic-items.png');await fixture.raw(`screenshot ${imagePath}`);
  const image=screenshot(imagePath),scale=Math.round(image.width/ui.gui_size[0]);
  assert(scale>=1&&Math.abs(image.width/ui.gui_size[0]-scale)<0.01,'integer GUI scale');
  const crop=(index:number)=>{
    const node=ui.widgets.find((w:any)=>w.id===`slot-${index}`);assert(node?.bounds,'visible hotbar slot');
    const out:number[]=[];
    for(let y=0;y<16;y++)for(let x=0;x<16;x++){
      const px=Math.floor((node.bounds[0]+x+0.5)*scale),py=Math.floor((node.bounds[1]+y+0.5)*scale);
      assert(px>=0&&py>=0&&px<image.width&&py<image.height);const offset=(py*image.width+px)*4;
      out.push(...image.pixels.subarray(offset,offset+4));
    }
    return out;
  };
  const pixelsDiffer=(a:number[],b:number[])=>a.reduce((n,v,i)=>n+(i%4!==3&&v!==b[i]?1:0),0);
  assert(pixelsDiffer(crop(0),crop(1))>0,'P2P frequency changes the Memory Card LED mesh');
  assert(pixelsDiffer(crop(2),crop(3))>0,'wrapped water and lava show different real fluid sprites');
  assert.deepEqual(crop(4),crop(5),'wrapped item key draws the contained diamond');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('offscreen Memory Card LEDs and wrapped item/fluid keys render from their real payloads');
  fixture.report('passed');
} catch(error) {fixture.report('failed',error);throw error;}
finally {await fixture.close();}
