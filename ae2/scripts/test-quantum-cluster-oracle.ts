/**
 * Compare the pinned, unmodified QuantumCluster and Locatables Java classes
 * with the standalone native registry using real callback order.
 * AE2_REFERENCE_DIR=/path/to/Applied-Energistics-2 bun ae2/scripts/test-quantum-cluster-oracle.ts
 * Requires a JDK and Cargo. Minecraft, grid, and event-bus types are stubs.
 */
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const ref = process.env.AE2_REFERENCE_DIR;
assert(ref, 'Set AE2_REFERENCE_DIR to the pinned AE2 source checkout');
const source = (name: string) => {
  const path = resolve(ref, 'src/main/java', name);
  assert(existsSync(path), `Missing source class: ${path}`);
  return path;
};
const actual = [
  source('appeng/me/cluster/implementations/QuantumCluster.java'),
  source('appeng/me/cluster/IAECluster.java'),
  source('appeng/me/service/helpers/ConnectionWrapper.java'),
  source('appeng/api/features/Locatables.java'),
  source('appeng/api/networking/security/IActionHost.java'),
];
const workRoot = join(root, '.local'); mkdirSync(workRoot, { recursive: true });
const work = mkdtempSync(join(workRoot, 'quantum-cluster-oracle-'));
const files: string[] = [];
function put(path: string, content: string) {
  const target = join(work, path); mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, content); files.push(target);
}
put('org/jetbrains/annotations/Nullable.java', `package org.jetbrains.annotations; public @interface Nullable {}`);
put('net/minecraft/core/BlockPos.java', `package net.minecraft.core;
public record BlockPos(int x, int y, int z) { public BlockPos immutable() { return this; } }`);
put('net/minecraft/world/level/block/entity/BlockEntity.java', `package net.minecraft.world.level.block.entity; public class BlockEntity {}`);
put('net/minecraft/world/level/Level.java', `package net.minecraft.world.level;
import java.util.*; import net.minecraft.core.BlockPos; import net.minecraft.world.level.block.entity.BlockEntity;
public class Level {
  private final Map<BlockPos,BlockEntity> entities=new HashMap<>();
  public final Server server=new Server(this);
  public boolean isClientSide() { return false; }
  public boolean hasChunkAt(BlockPos p) { return true; }
  public Server getServer() { return server; }
  public Object dimension() { return this; }
  public BlockEntity getBlockEntity(BlockPos p) { return entities.get(p); }
  public void put(BlockPos p, BlockEntity e) { entities.put(p,e); }
  public static class Server { final Level level; Server(Level l) {level=l;} public Level getLevel(Object key) {return level;} }
}`);
put('net/neoforged/bus/api/SubscribeEvent.java', `package net.neoforged.bus.api; public @interface SubscribeEvent {}`);
put('net/neoforged/neoforge/common/NeoForge.java', `package net.neoforged.neoforge.common;
public class NeoForge { public static final Bus EVENT_BUS=new Bus(); public static class Bus {
  public void register(Object x) {} public void unregister(Object x) {}
} }`);
put('net/neoforged/neoforge/event/level/LevelEvent.java', `package net.neoforged.neoforge.event.level;
import net.minecraft.world.level.Level;
public class LevelEvent { public static class Unload { public Level getLevel() { return null; } } }`);
put('appeng/api/networking/IGridNode.java', `package appeng.api.networking; public interface IGridNode {}`);
put('appeng/api/networking/IGridConnection.java', `package appeng.api.networking;
public interface IGridConnection { IGridNode a(); IGridNode b(); void destroy(); }`);
put('appeng/api/networking/GridHelper.java', `package appeng.api.networking;
import java.util.*;
public class GridHelper {
  public static final Set<String> edges=new TreeSet<>();
  public static IGridConnection createConnection(IGridNode a, IGridNode b) {
    var names=new String[]{a.toString(), b.toString()}; Arrays.sort(names);
    var key=names[0]+"-"+names[1]; edges.add(key);
    return new IGridConnection() {
      public IGridNode a(){return a;} public IGridNode b(){return b;}
      public void destroy(){edges.remove(key);}
    };
  }
}`);
put('appeng/core/AELog.java', `package appeng.core; public class AELog { public static void warn(String s,Object... a) {} }`);
put('appeng/me/cluster/MBCalculator.java', `package appeng.me.cluster;
public class MBCalculator { public static void setModificationInProgress(Object x) {} }`);
put('appeng/util/iterators/ChainedIterator.java', `package appeng.util.iterators;
import java.util.*;
public class ChainedIterator<T> implements Iterator<T> {
  private final Iterator<T> it;
  @SafeVarargs public ChainedIterator(T... values){it=Arrays.asList(values).iterator();}
  public boolean hasNext(){return it.hasNext();} public T next(){return it.next();}
}`);
put('appeng/blockentity/qnb/QuantumBridgeBlockEntity.java', `package appeng.blockentity.qnb;
import net.minecraft.core.BlockPos; import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.entity.BlockEntity;
import appeng.api.networking.IGridNode; import appeng.me.cluster.implementations.QuantumCluster;
public class QuantumBridgeBlockEntity extends BlockEntity {
  public final Level level; public final BlockPos pos; public final Node node;
  public boolean live=true; public long frequency; public int worldUpdates=0; public QuantumCluster cluster;
  public QuantumBridgeBlockEntity(Level l,BlockPos p,String name,long f){level=l;pos=p;node=new Node(name);frequency=f;}
  public Level getLevel(){return level;} public BlockPos getBlockPos(){return pos;}
  public long getQEFrequency(){return frequency;} public IGridNode getGridNode(){return live?node:null;}
  public Proxy getMainNode(){return new Proxy();} public class Proxy {public IGridNode getNode(){return getGridNode();}}
  public void updateStatus(QuantumCluster c,byte flags,boolean affectWorld){cluster=c;if(affectWorld)worldUpdates++;}
  // Same call order as QuantumBridgeBlockEntity.disconnect(false).
  public void unload(){if(cluster!=null){cluster.setUpdateStatus(false);cluster.destroy();}cluster=null;}
  public static class Node implements IGridNode { final String name; Node(String n){name=n;} public String toString(){return name;} }
}`);
put('appeng/me/cluster/implementations/Oracle.java', `package appeng.me.cluster.implementations;
import java.lang.reflect.Field; import net.minecraft.core.BlockPos; import net.minecraft.world.level.Level;
import appeng.api.networking.GridHelper; import appeng.blockentity.qnb.QuantumBridgeBlockEntity;
public class Oracle {
  static Level level=new Level(); static QuantumCluster a,b,c;
  static QuantumBridgeBlockEntity beA,beB,beC;
  static QuantumCluster form(QuantumBridgeBlockEntity center) {
    var q=new QuantumCluster(center.pos,center.pos); q.setCenter(center); center.cluster=q;
    var ring=q.getRing();
    for(int i=0;i<8;i++) {ring[i]=new QuantumBridgeBlockEntity(level,new BlockPos(center.pos.x(),i+1,0),"r",0);ring[i].cluster=q;}
    level.put(center.pos,center); return q;
  }
  static String sides(QuantumCluster q) throws Exception {
    if(q==null || q.isDestroyed())return "-";
    Field x=QuantumCluster.class.getDeclaredField("thisSide"),y=QuantumCluster.class.getDeclaredField("otherSide");
    x.setAccessible(true);y.setAccessible(true);return x.getLong(q)+"/"+y.getLong(q);
  }
  static void emit(String label) throws Exception {
    System.out.println(label+" A="+sides(a)+" B="+sides(b)+" C="+sides(c)+" links="+
      (GridHelper.edges.isEmpty()?"-":String.join(",",GridHelper.edges)));
  }
  public static void main(String[] args) throws Exception {
    beC=new QuantumBridgeBlockEntity(level,new BlockPos(20,0,0),"C",42);c=form(beC);c.updateStatus(true);emit("first");
    beA=new QuantumBridgeBlockEntity(level,new BlockPos(0,0,0),"A",42);beA.live=false;a=form(beA);a.updateStatus(true);emit("not-ready");
    beA.live=true;emit("ready-only");a.updateStatus(true);emit("paired");
    beB=new QuantumBridgeBlockEntity(level,new BlockPos(10,0,0),"B",42);b=form(beB);b.updateStatus(true);emit("third");
    beA.unload();if(beA.worldUpdates!=0)throw new AssertionError("unload affected world");emit("unload");
    beA=new QuantumBridgeBlockEntity(level,new BlockPos(0,0,0),"A",42);a=form(beA);a.updateStatus(true);emit("reload");
  }
}`);
put('native/Cargo.toml', `[package]
name = "quantum-cluster-oracle"
version = "0.0.0"
edition = "2021"
[dependencies]
ae2-network-core = { path = ${JSON.stringify(join(root, 'crates/ae2-network-core'))} }
`);
put('native/src/main.rs', `use ae2_network_core::{NodeIdentity,quantum_registry::QuantumRegistry};
fn id(n:u64)->NodeIdentity{NodeIdentity{host:n,part:None,outer:false}}
fn sides(r:&QuantumRegistry,n:u64)->String{r.sides(id(n)).map(|(a,b)|format!("{a}/{b}")).unwrap_or("-".into())}
fn emit(label:&str,r:&QuantumRegistry){
 let links:Vec<_>=r.links().map(|(a,b)|format!("{}-{}",match a.host{1=>"A",2=>"B",_=>"C"},match b.host{1=>"A",2=>"B",_=>"C"})).collect();
 println!("{label} A={} B={} C={} links={}",sides(r,1),sides(r,2),sides(r,3),if links.is_empty(){"-".into()}else{links.join(",")});
}
fn main(){
 let mut r=QuantumRegistry::default();
 r.form(id(3)).unwrap();r.update(id(3),42).unwrap();emit("first",&r);
 r.form_with_node(id(1),false).unwrap();r.update(id(1),42).unwrap();emit("not-ready",&r);
 r.set_node_live(id(1),true).unwrap();emit("ready-only",&r);r.update(id(1),42).unwrap();emit("paired",&r);
 r.form(id(2)).unwrap();r.update(id(2),42).unwrap();emit("third",&r);
 r.destroy(id(1)).unwrap();emit("unload",&r);
 r.form(id(1)).unwrap();r.update(id(1),42).unwrap();emit("reload",&r);
}`);

function run(cmd: string[]): string {
  const child = Bun.spawnSync({ cmd, cwd: root, stdout: 'pipe', stderr: 'pipe' });
  assert.equal(child.exitCode, 0, `${cmd[0]} failed:\n${child.stderr.toString()}`);
  return child.stdout.toString().trim();
}
run(['javac', '-d', work, ...files.filter(file => file.endsWith('.java')), ...actual]);
const java = run(['java', '-cp', work, 'appeng.me.cluster.implementations.Oracle']);
const native = run(['cargo', 'run', '--quiet', '--offline', '--manifest-path', join(work, 'native/Cargo.toml'), '--target-dir', join(work, 'target')]);
assert.equal(native, java, 'native quantum callback results differ from the pinned Java classes');
console.log(`PASS actual AE2 QuantumCluster and Locatables match native across ${java.split('\n').length} readiness, registration, unload, and reload states`);
