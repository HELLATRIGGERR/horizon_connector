/** Run the actual pinned AE2 channel calculation against synthetic native graphs. */
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { root } from './common';
import { requirePinnedReference } from './reference';
const reference=resolve(process.env.AE2_REFERENCE_DIR??join(root,'ae2-reference'));requirePinnedReference(reference);
const source=(path:string)=>readFileSync(join(reference,'src/main/java/appeng',path),'utf8');
function method(text:string,signature:string):string {
  const start=text.indexOf(signature);assert(start>=0,signature);let at=text.indexOf('{',start),depth=0;
  for(;at<text.length;at++){if(text[at]==='{')depth++;else if(text[at]==='}'&&--depth===0)return text.slice(start,at+1);}
  throw new Error('Unterminated source method '+signature);
}
const dir=mkdtempSync(join(tmpdir(),'ae2-pathing-oracle-'));
const clean=(text:string)=>text.replace(/^package .*;\s*$/gm,'').replace(/^import .*;\s*$/gm,'').replaceAll('@Nullable','');
writeFileSync(join(dir,'PathingCalculation.java'),'import java.util.*;\n'+clean(source('me/pathfinding/PathingCalculation.java')));
writeFileSync(join(dir,'IPathItem.java'),clean(source('me/pathfinding/IPathItem.java')));
const node=source('me/GridNode.java'),edge=source('me/GridConnection.java');
const methods=['public void setControllerRoute(IPathItem fast)','public int getMaxChannels()','public Iterable<IPathItem> getPossibleOptions()','public int propagateChannelsUpwards(boolean consumesChannel)','public void incrementChannelCount(int usedChannels)'].map(signature=>method(node,signature)).join('\n');
const edges=['public GridNode getControllerRoute()','public void setControllerRoute(IPathItem fast)','public int getMaxChannels()','public Iterable<IPathItem> getPossibleOptions()','public boolean hasFlag(GridFlags flag)','public int propagateChannelsUpwards()'].map(signature=>method(edge,signature)).join('\n');
const template=readFileSync(join(root,'crates/ae2-network-core/tests/oracle/PathingOracle.java'),'utf8');
writeFileSync(join(dir,'PathingOracle.java'),template.replace('/* NODE_METHODS */',methods).replace('/* EDGE_METHODS */',edges));
let seed=0x00ae2001;const random=(n:number)=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed%n;};
const cases:{types:number[];groups:number[];edges:number[][]}[]=[];
for(let c=0;c<128;c++){
  const n=16+random(33),types=[0,...Array.from({length:n-1},()=>1+random(3))],groups=Array(n).fill(-1),edges=new Set<string>();
  const link=(a:number,b:number)=>{if(a!==b)edges.add([Math.min(a,b),Math.max(a,b)].join(' '));};
  for(let i=1;i<n;i++)link(i,random(i));for(let i=0;i<n;i++)for(let j=i+1;j<n;j++)if(random(18)===0)link(i,j);
  if(c%4===0){types[n-1]=4;types[n-2]=4;groups[n-1]=n-2;groups[n-2]=n-2;}
  cases.push({types,groups,edges:[...edges].map(line=>line.split(' ').map(Number)).sort((a,b)=>a[0]-b[0]||a[1]-b[1])});
}
const input=[String(cases.length),...cases.flatMap(c=>[`${c.types.length} ${c.edges.length}`,...c.types.map((type,i)=>`${type} ${c.groups[i]}`),...c.edges.map(pair=>pair.join(' '))])].join('\n');
writeFileSync(join(dir,'graphs.txt'),input);
function run(command:string[],feed?:string):string {
  const result=Bun.spawnSync(command,{cwd:root,stdin:feed===undefined?undefined:new TextEncoder().encode(feed),stdout:'pipe',stderr:'pipe'});
  if(result.exitCode!==0)process.stderr.write(result.stderr);assert.equal(result.exitCode,0,command.join(' '));return result.stdout.toString();
}
const javaHome=process.env.JAVA_HOME;
run([javaHome?join(javaHome,'bin/javac'):'javac','-d',dir,join(dir,'PathingOracle.java'),join(dir,'IPathItem.java'),join(dir,'PathingCalculation.java')]);
const expected=JSON.parse(run([javaHome?join(javaHome,'bin/java'):'java','-cp',dir,'PathingOracle'],input));
const actual=JSON.parse(run(['cargo','+1.95.0','run','--locked','--offline','--quiet','--manifest-path',join(root,'crates/ae2-network-core/Cargo.toml'),'--example','pathing_oracle'],input));
for(let i=0;i<cases.length;i++)assert.deepEqual(actual[i],expected[i],`source pathing graph ${i}: ${JSON.stringify(cases[i])}`);
console.log(`PASS ${cases.length} synthetic graphs against pinned AE2 PathingCalculation, GridNode and GridConnection methods. This is not live-world evidence.`);
