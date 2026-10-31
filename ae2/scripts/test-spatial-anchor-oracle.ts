/**
 * Compare the pinned Java StatisticsService and GridChunkEvent against the
 * native ordered chunk observer. The minimal host stubs reproduce Grid.add /
 * Grid.remove owner ordering; no boot or power state is invented.
 * AE2_REFERENCE_DIR=/path/to/Applied-Energistics-2 bun ae2/scripts/test-spatial-anchor-oracle.ts
 */
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const ref = process.env.AE2_REFERENCE_DIR;
assert(ref, 'Set AE2_REFERENCE_DIR to the pinned AE2 source checkout');
const actual = [
  'appeng/me/service/StatisticsService.java',
  'appeng/api/networking/events/statistics/GridChunkEvent.java',
  'appeng/api/networking/events/statistics/GridStatisticsEvent.java',
  'appeng/api/networking/events/GridEvent.java',
].map(name => {
  const file = resolve(ref, 'src/main/java', name);
  assert(existsSync(file), `Missing Java source: ${file}`); return file;
});
const production = join(root, 'crates/ae2-links-core/src/anchor_grid.rs');
const nativeSource = process.env.AE2_ANCHOR_ORACLE_SOURCE ?? (existsSync(production) ? production : join(root, '.local/anchor-nextwave-staging/anchor_grid.rs'));
assert(existsSync(nativeSource), `Missing native observer: ${nativeSource}`);
mkdirSync(join(root, '.local'), { recursive: true });
const work = mkdtempSync(join(root, '.local/spatial-anchor-oracle-'));
const files: string[] = [];
function put(name: string, content: string) {
  const target = join(work, name); mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, content); files.push(target);
}
put('org/jetbrains/annotations/Nullable.java', `package org.jetbrains.annotations; public @interface Nullable {}`);
put('com/google/common/collect/Multiset.java', `package com.google.common.collect;
import java.util.Set; public interface Multiset<T> {
  boolean contains(Object x); boolean add(T x); boolean remove(Object x); boolean isEmpty(); Set<T> elementSet();
}`);
put('com/google/common/collect/HashMultiset.java', `package com.google.common.collect;
import java.util.*; public class HashMultiset<T> implements Multiset<T> {
  private final Map<T,Integer> values=new HashMap<>(); public static <T> HashMultiset<T> create(){return new HashMultiset<>();}
  public boolean contains(Object x){return values.containsKey(x);} public boolean isEmpty(){return values.isEmpty();}
  public boolean add(T x){values.merge(x,1,Integer::sum);return true;}
  @SuppressWarnings("unchecked") public boolean remove(Object x){var n=values.get(x);if(n==null)return false;
    if(n==1)values.remove(x);else values.put((T)x,n-1);return true;}
  public Set<T> elementSet(){return values.keySet();}
}`);
put('com/google/gson/stream/JsonWriter.java', `package com.google.gson.stream; public class JsonWriter {}`);
put('net/minecraft/core/BlockPos.java', `package net.minecraft.core; public record BlockPos(int x,int y,int z) {}`);
put('net/minecraft/core/HolderLookup.java', `package net.minecraft.core; public class HolderLookup { public interface Provider {} }`);
put('net/minecraft/nbt/CompoundTag.java', `package net.minecraft.nbt; public class CompoundTag {}`);
put('net/minecraft/server/level/ServerLevel.java', `package net.minecraft.server.level;
public class ServerLevel { public final int id; public ServerLevel(int id){this.id=id;}
  public Dimension dimension(){return new Dimension();} public class Dimension {public Object location(){return id;}}
}`);
put('net/minecraft/world/level/ChunkPos.java', `package net.minecraft.world.level;
import net.minecraft.core.BlockPos; public record ChunkPos(int x,int z) {
  public ChunkPos(BlockPos p){this(p.x()>>4,p.z()>>4);}
}`);
put('appeng/api/networking/IGridNode.java', `package appeng.api.networking; public interface IGridNode {}`);
put('appeng/api/networking/IGrid.java', `package appeng.api.networking;
import appeng.api.networking.events.GridEvent; public interface IGrid { <T extends GridEvent> T postEvent(T event); }`);
put('appeng/api/networking/IGridService.java', `package appeng.api.networking; public interface IGridService {}`);
put('appeng/api/networking/IGridServiceProvider.java', `package appeng.api.networking;
import java.io.IOException; import net.minecraft.nbt.CompoundTag; import net.minecraft.core.HolderLookup; import com.google.gson.stream.JsonWriter;
public interface IGridServiceProvider {
  void addNode(IGridNode node, CompoundTag data); void removeNode(IGridNode node);
  void debugDump(JsonWriter writer, HolderLookup.Provider registries) throws IOException;
}`);
put('appeng/me/InWorldGridNode.java', `package appeng.me;
import appeng.api.networking.IGridNode; import net.minecraft.server.level.ServerLevel; import net.minecraft.core.BlockPos;
public class InWorldGridNode implements IGridNode {
  final ServerLevel level; final BlockPos pos;
  public InWorldGridNode(ServerLevel l, BlockPos p){level=l;pos=p;}
  public ServerLevel getLevel(){return level;} public BlockPos getLocation(){return pos;}
}`);
put('appeng/util/JsonStreamUtil.java', `package appeng.util;
import java.io.IOException; import java.util.Map; import com.google.gson.stream.JsonWriter;
public class JsonStreamUtil {
 public static void writeProperties(Map<String,Object> values, JsonWriter writer) throws IOException {}
 public static Object toJson(Object x){return x;}
}`);
put('Oracle.java', `import java.util.*;
import appeng.api.networking.*; import appeng.api.networking.events.GridEvent;
import appeng.api.networking.events.statistics.GridChunkEvent; import appeng.me.InWorldGridNode;
import appeng.me.service.StatisticsService; import net.minecraft.core.BlockPos; import net.minecraft.server.level.ServerLevel;
public class Oracle {
 static ServerLevel d0=new ServerLevel(0), d1=new ServerLevel(1);
 record Owner(int id,ServerLevel level) {}
 record Member(IGridNode node,Owner anchor) {}
 static class Plain implements IGridNode {}
 static class Grid implements IGrid {
  final Set<Owner> anchors=new LinkedHashSet<>(); final StatisticsService stats=new StatisticsService(this);
  public <T extends GridEvent> T postEvent(T event) {
   var e=(GridChunkEvent)event;
   for(var a:anchors) if(a.level==e.getLevel()) System.out.println((e instanceof GridChunkEvent.GridChunkAdded?"+":"-")
     +" "+a.id+" "+e.getLevel().id+" "+e.getChunkPos().x()+" "+e.getChunkPos().z());
   return event;
  }
  void add(Member x){if(x.anchor!=null)anchors.add(x.anchor);stats.addNode(x.node,null);}
  void remove(Member x){stats.removeNode(x.node);if(x.anchor!=null)anchors.remove(x.anchor);}
 }
 static Member node(ServerLevel d,int cx,boolean inWorld,int anchor){
  IGridNode n=inWorld?new InWorldGridNode(d,new BlockPos(cx*16,64,0)):new Plain();
  return new Member(n,anchor==0?null:new Owner(anchor,d));
 }
 public static void main(String[] args){
  var g1=new Grid();var g2=new Grid();
  var a=node(d0,0,true,1);var part=node(d0,1,false,0);var outer=node(d0,1,true,0);var second=node(d0,1,true,0);
  var b=node(d1,0,true,2);var moved=node(d1,5,true,0);
  g1.add(a);g1.add(part);g1.add(outer);g1.add(second);g1.remove(outer);g1.remove(second);
  g2.add(b);g1.add(moved);g1.remove(moved);g2.add(moved);g2.remove(moved);g1.remove(a);g2.remove(b);
 }
}`);
put('native.rs', `#[path = ${JSON.stringify(nativeSource)}] mod anchor_grid;
use anchor_grid::{ChunkObserver,Kind};
fn run(o:&mut ChunkObserver<u8>,id:u8,a:Option<u64>,b:Option<u64>) {
 for e in o.membership(id,a,b).unwrap(){
  println!("{} {} {} {} {}",if e.kind==Kind::Added{"+"}else{"-"},e.anchor,e.dimension,e.chunk.0,e.chunk.1);
 }
}
fn main(){
 let mut o=ChunkObserver::default();
 for (id,d,c,w,a) in [(1,0,0,true,true),(2,0,1,false,false),(3,0,1,true,false),(4,0,1,true,false),
                       (5,1,0,true,true),(6,1,5,true,false)] {
  o.register(id,d,(c,0),w,a).unwrap();
 }
 run(&mut o,1,None,Some(1));run(&mut o,2,None,Some(1));run(&mut o,3,None,Some(1));run(&mut o,4,None,Some(1));
 run(&mut o,3,Some(1),None);run(&mut o,4,Some(1),None);
 // Java owner B is labeled 2; the actual managed node identity is 5.
 // Keep native display labels in sync without replacing node identity.
 run(&mut o,5,None,Some(2));run(&mut o,6,None,Some(1));run(&mut o,6,Some(1),Some(2));
 run(&mut o,6,Some(2),None);run(&mut o,1,Some(1),None);run(&mut o,5,Some(2),None);
}`);
function run(cmd: string[]): string {
  const child = Bun.spawnSync({ cmd, cwd: root, stdout: 'pipe', stderr: 'pipe' });
  assert.equal(child.exitCode, 0, `${cmd[0]} failed:\n${child.stderr.toString()}`); return child.stdout.toString().trim();
}
run(['javac', '-d', work, ...files.filter(path => path.endsWith('.java')), ...actual]);
const java = run(['java', '-cp', work, 'Oracle']);
run(['rustc', '--edition', '2021', '-Awarnings', join(work, 'native.rs'), '-o', join(work, 'native')]);
const native = run([join(work, 'native')]).split('\n').map(line => line.replace(/^([+-]) 5 /, '$1 2 ')).join('\n');
assert.equal(native, java, 'native ordered anchor chunk callbacks differ from the pinned StatisticsService');
console.log(`PASS actual AE2 StatisticsService and chunk events match native across ${java.split('\n').length} owner callbacks`);
