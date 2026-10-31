/** Run the unchanged pinned Minecraft ArmorStandItem against bounded stubs.
 * Set AE2_MC_SOURCE_JAR to the 1.21.1 NeoForge sources jar; requires a JDK.
 * The hidden world fixture checks the native Formation Plane end to end.
 */
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const jar = process.env.AE2_MC_SOURCE_JAR;
assert(jar && existsSync(jar), 'Set AE2_MC_SOURCE_JAR to the pinned NeoForge sources jar');
mkdirSync(join(root, '.local'), { recursive: true });
const work = mkdtempSync(join(root, '.local/armor-formation-oracle-'));
const source = Bun.spawnSync({ cmd: ['unzip', '-p', jar, 'net/minecraft/world/item/ArmorStandItem.java'], stdout: 'pipe', stderr: 'pipe' });
assert.equal(source.exitCode, 0, source.stderr.toString());
assert(source.stdout.length > 0, 'The source jar did not contain ArmorStandItem.java');
const files: Record<string, string> = {
  'net/minecraft/world/item/ArmorStandItem.java': source.stdout.toString(),
  'net/minecraft/core/BlockPos.java': `package net.minecraft.core; public record BlockPos(int x,int y,int z) {}`,
  'net/minecraft/core/Direction.java': `package net.minecraft.core; public enum Direction {DOWN,UP,NORTH,SOUTH,WEST,EAST}`,
  'net/minecraft/world/phys/Vec3.java': `package net.minecraft.world.phys; import net.minecraft.core.BlockPos; public record Vec3(double x,double y,double z){ public static Vec3 atBottomCenterOf(BlockPos p){return new Vec3(p.x()+.5,p.y(),p.z()+.5);} }`,
  'net/minecraft/world/phys/AABB.java': `package net.minecraft.world.phys; public class AABB {}`,
  'net/minecraft/world/InteractionResult.java': `package net.minecraft.world; public enum InteractionResult {FAIL,SUCCESS; public static InteractionResult sidedSuccess(boolean client){return SUCCESS;}}`,
  'net/minecraft/world/item/Item.java': `package net.minecraft.world.item; import net.minecraft.world.InteractionResult; import net.minecraft.world.item.context.UseOnContext; public class Item { public static class Properties {} public Item(Properties p){} public InteractionResult useOn(UseOnContext c){return InteractionResult.FAIL;} }`,
  'net/minecraft/world/item/ItemStack.java': `package net.minecraft.world.item; public class ItemStack { public int count=2; public String name="Guardian"; public boolean small=true; public void shrink(int n){count-=n;} }`,
  'net/minecraft/world/item/context/UseOnContext.java': `package net.minecraft.world.item.context; import net.minecraft.core.*; import net.minecraft.world.item.ItemStack; import net.minecraft.world.level.Level; public class UseOnContext { public Direction face=Direction.UP; public Level level; public BlockPos pos=new BlockPos(0,64,0); public ItemStack stack=new ItemStack(); public float rotation; public Direction getClickedFace(){return face;} public Level getLevel(){return level;} public ItemStack getItemInHand(){return stack;} public Object getPlayer(){return null;} public float getRotation(){return rotation;} }`,
  'net/minecraft/world/item/context/BlockPlaceContext.java': `package net.minecraft.world.item.context; import net.minecraft.core.BlockPos; public class BlockPlaceContext { private final UseOnContext context; public BlockPlaceContext(UseOnContext c){context=c;} public BlockPos getClickedPos(){return context.pos;} }`,
  'net/minecraft/world/level/Level.java': `package net.minecraft.world.level; import java.util.*; import net.minecraft.world.phys.AABB; import net.minecraft.sounds.SoundSource; public class Level {public boolean isClientSide=false,blocked=false,occupied=false; public int sounds; public boolean noCollision(Object e,AABB b){return !blocked;} public List<Object> getEntities(Object e,AABB b){return occupied?List.of(new Object()):List.of();} public void playSound(Object p,double x,double y,double z,Object sound,SoundSource source,float vol,float pitch){sounds++;}}`,
  'net/minecraft/server/level/ServerLevel.java': `package net.minecraft.server.level; import net.minecraft.world.entity.decoration.ArmorStand; public class ServerLevel extends net.minecraft.world.level.Level { public ArmorStand stand; public int count; public void addFreshEntityWithPassengers(ArmorStand a){stand=a;count++;}}`,
  'net/minecraft/sounds/SoundEvents.java': `package net.minecraft.sounds; public class SoundEvents {public static final Object ARMOR_STAND_PLACE=new Object();}`,
  'net/minecraft/sounds/SoundSource.java': `package net.minecraft.sounds; public enum SoundSource {BLOCKS}`,
  'net/minecraft/world/level/gameevent/GameEvent.java': `package net.minecraft.world.level.gameevent; public class GameEvent {public static final GameEvent ENTITY_PLACE=new GameEvent();}`,
  'net/minecraft/util/Mth.java': `package net.minecraft.util; public class Mth {public static float wrapDegrees(float f){float r=f%360; if(r>=180)r-=360; if(r< -180)r+=360; return r;} public static int floor(float f){return (int)Math.floor(f);}}`,
  'net/minecraft/world/entity/MobSpawnType.java': `package net.minecraft.world.entity; public enum MobSpawnType {SPAWN_EGG}`,
  'net/minecraft/world/entity/decoration/ArmorStand.java': `package net.minecraft.world.entity.decoration; import net.minecraft.world.level.gameevent.GameEvent; public class ArmorStand {public double x,y,z; public float yaw; public String name; public boolean small; public int events; public double getX(){return x;} public double getY(){return y;} public double getZ(){return z;} public void moveTo(double x,double y,double z,float yaw,float pitch){this.x=x;this.y=y;this.z=z;this.yaw=yaw;} public void gameEvent(GameEvent event,Object player){events++;}}`,
  'net/minecraft/world/entity/EntityType.java': `package net.minecraft.world.entity; import java.util.function.Consumer; import net.minecraft.core.BlockPos; import net.minecraft.server.level.ServerLevel; import net.minecraft.world.item.ItemStack; import net.minecraft.world.entity.decoration.ArmorStand; import net.minecraft.world.phys.AABB; public class EntityType {public static final EntityType ARMOR_STAND=new EntityType(); public Dimensions getDimensions(){return new Dimensions();} public static class Dimensions {public AABB makeBoundingBox(double x,double y,double z){return new AABB();}} public static Consumer<ArmorStand> createDefaultStackConfig(ServerLevel level,ItemStack stack,Object player){return a->{a.name=stack.name;a.small=stack.small;};} public ArmorStand create(ServerLevel level,Consumer<ArmorStand> configure,BlockPos p,MobSpawnType type,boolean align,boolean invert){ArmorStand a=new ArmorStand();a.x=p.x()+.5;a.y=p.y();a.z=p.z()+.5;configure.accept(a);return a;}}`,
  'Oracle.java': `import net.minecraft.core.*; import net.minecraft.server.level.ServerLevel; import net.minecraft.world.*; import net.minecraft.world.item.*; import net.minecraft.world.item.context.UseOnContext;
public class Oracle {
 static void check(boolean b,String why){if(!b)throw new AssertionError(why);}
 static UseOnContext ctx(ServerLevel l,Direction face,float rotation){var c=new UseOnContext();c.level=l;c.face=face;c.rotation=rotation;return c;}
 public static void main(String[] args){var item=new ArmorStandItem(new Item.Properties());
  var l=new ServerLevel();var c=ctx(l,Direction.DOWN,0);check(item.useOn(c)==InteractionResult.FAIL&&c.stack.count==2&&l.count==0,"downward face");
  c=ctx(l,Direction.UP,0);l.blocked=true;check(item.useOn(c)==InteractionResult.FAIL&&c.stack.count==2&&l.count==0,"block collision");
  l.blocked=false;l.occupied=true;check(item.useOn(c)==InteractionResult.FAIL&&c.stack.count==2&&l.count==0,"entity collision");l.occupied=false;
  float[] rotations={0,90,180,270,-90,-90};float[] yaws={-180,-90,0,90,90,90};
  for(int i=0;i<rotations.length;i++){l=new ServerLevel();c=ctx(l,Direction.UP,rotations[i]);check(item.useOn(c)==InteractionResult.SUCCESS,"success "+i);check(c.stack.count==1&&l.count==1&&l.sounds==1&&l.stand.events==1,"physical effects "+i);check(l.stand.yaw==yaws[i],"yaw "+i);check(l.stand.small&&l.stand.name.equals("Guardian"),"stack config "+i);}
  System.out.println("PASS unchanged Minecraft ArmorStandItem face, collision, count, yaw and spawn behavior");
 }} `,
};
const sources: string[] = [];
for (const [name, text] of Object.entries(files)) {
  const path = join(work, name); mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, text + '\n'); sources.push(path);
}
for (const cmd of [['javac', '-d', work, ...sources], ['java', '-cp', work, 'Oracle']]) {
  const result = Bun.spawnSync({ cmd, cwd: root, stdout: 'pipe', stderr: 'pipe' });
  assert.equal(result.exitCode, 0, `${cmd[0]} failed:\n${result.stderr.toString()}`);
  if (result.stdout.length) console.log(result.stdout.toString().trim());
}
