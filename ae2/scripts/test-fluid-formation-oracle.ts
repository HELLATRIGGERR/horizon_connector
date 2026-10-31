/** Exercise the unchanged AE2 FluidPlacementStrategy with minimal world stubs.
 * AE2_REFERENCE_DIR must name the pinned 19.2.18 source; requires a JDK.
 * The corresponding native world fixtures assert the same physical outcomes.
 */
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const ref = process.env.AE2_REFERENCE_DIR;
assert(ref, 'Set AE2_REFERENCE_DIR to the pinned source checkout');
const actual = resolve(ref, 'src/main/java/appeng/parts/automation/FluidPlacementStrategy.java');
assert(existsSync(actual), `Missing source class: ${actual}`);
mkdirSync(join(root, '.local'), { recursive: true });
const work = mkdtempSync(join(root, '.local/fluid-formation-oracle-'));
const files: Record<string,string> = {
  'org/jetbrains/annotations/Nullable.java': `package org.jetbrains.annotations; public @interface Nullable {}`,
  'net/minecraft/core/BlockPos.java': `package net.minecraft.core; public record BlockPos(int x,int y,int z) { public int getX(){return x;} public int getY(){return y;} public int getZ(){return z;} }`,
  'net/minecraft/core/Direction.java': `package net.minecraft.core; public enum Direction { EAST }`,
  'net/minecraft/core/particles/ParticleTypes.java': `package net.minecraft.core.particles; public class ParticleTypes { public static final Object LARGE_SMOKE=new Object(); }`,
  'net/minecraft/sounds/SoundEvent.java': `package net.minecraft.sounds; public class SoundEvent {}`,
  'net/minecraft/sounds/SoundEvents.java': `package net.minecraft.sounds; public class SoundEvents { public static final SoundEvent BUCKET_EMPTY=new SoundEvent(), BUCKET_EMPTY_LAVA=new SoundEvent(), FIRE_EXTINGUISH=new SoundEvent(); }`,
  'net/minecraft/sounds/SoundSource.java': `package net.minecraft.sounds; public enum SoundSource { BLOCKS }`,
  'net/minecraft/tags/FluidTags.java': `package net.minecraft.tags; public class FluidTags { public static final String WATER="water", LAVA="lava"; }`,
  'net/minecraft/world/level/block/entity/BlockEntity.java': `package net.minecraft.world.level.block.entity; public class BlockEntity {}`,
  'net/minecraft/world/level/gameevent/GameEvent.java': `package net.minecraft.world.level.gameevent; public class GameEvent { public static final GameEvent FLUID_PLACE=new GameEvent(); public static class Context { public static Context of(Object a,Object b){return new Context();} } }`,
  'net/minecraft/world/level/block/Block.java': `package net.minecraft.world.level.block; public class Block { public static final int UPDATE_ALL_IMMEDIATE=11; }`,
  'net/minecraft/world/level/block/LiquidBlockContainer.java': `package net.minecraft.world.level.block; import net.minecraft.core.BlockPos; import net.minecraft.world.level.Level; import net.minecraft.world.level.material.*; import net.minecraft.world.level.block.state.BlockState; public interface LiquidBlockContainer { boolean canPlaceLiquid(Object player,Level level,BlockPos pos,BlockState state,Fluid fluid); boolean placeLiquid(Level level,BlockPos pos,BlockState state,FluidState fluid); }`,
  'net/minecraft/world/level/block/Slab.java': `package net.minecraft.world.level.block; import net.minecraft.core.BlockPos; import net.minecraft.world.level.Level; import net.minecraft.world.level.material.*; import net.minecraft.world.level.block.state.BlockState; public class Slab extends Block implements LiquidBlockContainer { public boolean canPlaceLiquid(Object p,Level l,BlockPos pos,BlockState s,Fluid f){return f==Fluids.WATER;} public boolean placeLiquid(Level l,BlockPos p,BlockState s,FluidState f){if(s.wet)return false; s.wet=true; return true;} }`,
  'net/minecraft/world/level/block/state/BlockState.java': `package net.minecraft.world.level.block.state; import net.minecraft.world.level.block.Block; import net.minecraft.world.level.material.*; public class BlockState { public final Block block; public final boolean air,replaceable,liquid; public boolean wet; public final FluidState fluid; public BlockState(Block b,boolean a,boolean r,boolean l,FluidState f){block=b;air=a;replaceable=r;liquid=l;fluid=f;} public Block getBlock(){return block;} public boolean isAir(){return air;} public boolean canBeReplaced(Fluid f){return replaceable;} public boolean liquid(){return liquid;} public FluidState getFluidState(){return fluid;} }`,
  'net/minecraft/world/level/material/FluidState.java': `package net.minecraft.world.level.material; import net.minecraft.world.level.block.Block; import net.minecraft.world.level.block.state.BlockState; public class FluidState { public final boolean source; public final BlockState block; public FluidState(boolean source){this.source=source;block=new BlockState(new Block(),false,true,true,this);} public BlockState createLegacyBlock(){return block;} public boolean isSource(){return source;} }`,
  'net/minecraft/world/level/material/Fluid.java': `package net.minecraft.world.level.material; public class Fluid { public final String family; private final FluidState state; public Fluid(String family,boolean source){this.family=family;state=new FluidState(source);} public FluidState defaultFluidState(){return state;} public boolean is(String tag){return family.equals(tag);} }`,
  'net/minecraft/world/level/material/FlowingFluid.java': `package net.minecraft.world.level.material; public class FlowingFluid extends Fluid { public FlowingFluid(String family,boolean source){super(family,source);} public FluidState getSource(boolean falling){return family.equals("water")?Fluids.WATER.defaultFluidState():Fluids.LAVA.defaultFluidState();} }`,
  'net/minecraft/world/level/material/Fluids.java': `package net.minecraft.world.level.material; public class Fluids { public static final FlowingFluid WATER=new FlowingFluid("water",true), LAVA=new FlowingFluid("lava",true); }`,
  'net/minecraft/world/level/Level.java': `package net.minecraft.world.level; import java.util.Random; import net.minecraft.core.BlockPos; import net.minecraft.sounds.*; import net.minecraft.world.level.block.Block; import net.minecraft.world.level.block.state.BlockState; import net.minecraft.world.level.material.FluidState; import net.minecraft.world.level.gameevent.GameEvent; public class Level { public final Random random=new Random(42); public int sounds,events; public boolean warm; public BlockState state=air(); public static BlockState air(){return new BlockState(new Block(),true,true,false,new FluidState(false));} public record DimensionType(boolean ultraWarm) {} public DimensionType dimensionType(){return new DimensionType(warm);} public BlockState getBlockState(BlockPos p){return state;} public boolean setBlock(BlockPos p,BlockState s,int f){if(state==s)return false; state=s;return true;} public void destroyBlock(BlockPos p,boolean drops){state=air();} public void playSound(Object o,BlockPos p,SoundEvent s,SoundSource c,float v,float pitch){sounds++;} public void gameEvent(GameEvent e,BlockPos p,GameEvent.Context c){events++;} public void addParticle(Object p,double x,double y,double z,double dx,double dy,double dz){} }`,
  'net/minecraft/server/level/ServerLevel.java': `package net.minecraft.server.level; public class ServerLevel extends net.minecraft.world.level.Level {}`,
  'appeng/api/behaviors/PlacementStrategy.java': `package appeng.api.behaviors; import appeng.api.config.Actionable; import appeng.api.stacks.AEKey; public interface PlacementStrategy {void clearBlocked(); long placeInWorld(AEKey k,long n,Actionable a,boolean entity);}`,
  'appeng/api/config/Actionable.java': `package appeng.api.config; public enum Actionable { SIMULATE,MODULATE }`,
  'appeng/api/stacks/AEKey.java': `package appeng.api.stacks; public class AEKey {}`,
  'appeng/api/stacks/AEFluidKey.java': `package appeng.api.stacks; import java.util.*; import net.minecraft.world.level.material.Fluid; public class AEFluidKey extends AEKey { public static final long AMOUNT_BLOCK=1000; private final Fluid fluid; private final boolean patched; public AEFluidKey(Fluid f,boolean p){fluid=f;patched=p;} public Fluid getFluid(){return fluid;} public Stack toStack(int n){return new Stack(patched);} public record Stack(boolean patched){public Set<String> getComponentsPatch(){return patched?Set.of("x"):Set.of();}} }`,
  'appeng/util/Platform.java': `package appeng.util; import java.util.UUID; import net.minecraft.server.level.ServerLevel; public class Platform { public static Object getFakePlayer(ServerLevel l,UUID id){return null;} }`,
  'Oracle.java': `import appeng.parts.automation.FluidPlacementStrategy;
import appeng.api.config.Actionable; import appeng.api.stacks.*; import net.minecraft.core.*;
import net.minecraft.server.level.ServerLevel; import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.*; import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState; import net.minecraft.world.level.material.*;
public class Oracle {
 static void eq(long a,long b,String message){if(a!=b)throw new AssertionError(message+": "+a+" != "+b);}
 static void ok(boolean v,String message){if(!v)throw new AssertionError(message);}
 static FluidPlacementStrategy plane(ServerLevel l){return new FluidPlacementStrategy(l,new BlockPos(0,1,0),Direction.EAST,new BlockEntity(),null);}
 public static void main(String[] args){
  var level=new ServerLevel(); var p=plane(level); var water=new AEFluidKey(Fluids.WATER,false); var lava=new AEFluidKey(Fluids.LAVA,false);
  eq(p.placeInWorld(water,1000,Actionable.SIMULATE,false),1000,"simulate"); ok(level.state.isAir(),"simulate mutated world");
  eq(p.placeInWorld(water,1000,Actionable.MODULATE,false),1000,"source"); ok(level.state==Fluids.WATER.defaultFluidState().createLegacyBlock(),"wrong water");
  eq(p.placeInWorld(water,1000,Actionable.SIMULATE,false),0,"same source");
  var slab=new BlockState(new Slab(),false,false,false,new FluidState(false)); level.state=slab;
  eq(p.placeInWorld(water,1000,Actionable.SIMULATE,false),0,"failed placement cached");
  p.clearBlocked(); eq(p.placeInWorld(water,1000,Actionable.MODULATE,false),1000,"dry slab"); ok(slab.wet,"slab not waterlogged");
  eq(p.placeInWorld(water,1000,Actionable.MODULATE,false),1000,"already wet slab"); ok(slab.wet,"wet slab changed");
  level.state=Level.air(); eq(p.placeInWorld(water,999,Actionable.MODULATE,false),0,"partial bucket");
  eq(p.placeInWorld(water,1000,Actionable.MODULATE,true),0,"entity mode");
  eq(p.placeInWorld(new AEFluidKey(Fluids.WATER,true),1000,Actionable.MODULATE,false),0,"patched fluid");
  eq(p.placeInWorld(new AEFluidKey(new Fluid("other",true),false),1000,Actionable.MODULATE,false),0,"non-flowing fluid");
  eq(p.placeInWorld(lava,1000,Actionable.MODULATE,false),1000,"lava"); ok(level.state==Fluids.LAVA.defaultFluidState().createLegacyBlock(),"wrong lava");
  var cacheLevel=new ServerLevel(); cacheLevel.state=new BlockState(new Block(),false,false,false,new FluidState(false)); var cp=plane(cacheLevel);
  eq(cp.placeInWorld(water,1000,Actionable.SIMULATE,false),0,"stone rejects water"); cacheLevel.state=Level.air();
  eq(cp.placeInWorld(lava,1000,Actionable.SIMULATE,false),1000,"water blockage does not block lava");
  eq(cp.placeInWorld(water,1000,Actionable.SIMULATE,false),0,"water remains cached"); cp.clearBlocked();
  eq(cp.placeInWorld(water,1000,Actionable.SIMULATE,false),1000,"clearBlocked permits retry");
  var nether=new ServerLevel(); nether.warm=true; var np=plane(nether); eq(np.placeInWorld(water,1000,Actionable.MODULATE,false),1000,"nether water"); ok(nether.state.isAir(),"nether retained water"); eq(nether.events,0,"evaporation posts no fluid-place event"); eq(nether.sounds,1,"evaporation sound");
  System.out.println("PASS unchanged AE2 FluidPlacementStrategy simulation, cache, waterlogged, gating, lava and ultrawarm cases");
 } }`,
};
const sources: string[] = [];
for (const [name, source] of Object.entries(files)) {
  const path = join(work, name); mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, source + '\n'); sources.push(path);
}
function run(cmd: string[]): string {
  const result = Bun.spawnSync({ cmd, cwd: root, stdout: 'pipe', stderr: 'pipe' });
  assert.equal(result.exitCode, 0, `${cmd[0]} failed:\n${result.stderr.toString()}`);
  return result.stdout.toString().trim();
}
run(['javac', '-d', work, ...sources, actual]);
console.log(run(['java', '-cp', work, 'Oracle']));
