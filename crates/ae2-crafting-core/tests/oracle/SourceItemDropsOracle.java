/* Test-only stubs around AEItemKey.addDrops from AE2 19.2.18, LGPL-3.0-or-later.
 * These do not model Minecraft entities, randomized spawn positions or saving.
 */
import java.util.*;
class Level {}
class BlockPos {}
class AELog {static void warn(String message,Object... args){}}
record ItemStack(String item,int count) {String getItem(){return item;}}
class AEItemKey {
    final ItemStack stack;
    final int maximum;
    AEItemKey(String item,int maximum){stack=new ItemStack(item,1);this.maximum=maximum;}
    int getMaxStackSize(){return maximum;}
    ItemStack toStack(int count){return new ItemStack(stack.item(),count);}
    // ACTUAL_ADD_DROPS
}
public class SourceItemDropsOracle {
    static void check(boolean value,String name){if(!value)throw new AssertionError(name);System.out.println(name+"=1");}
    public static void main(String[] args){
        var level=new Level();var position=new BlockPos();var large=new AEItemKey("stone",16);var small=new AEItemKey("dirt",8);
        var blockAdditional=new ArrayList<ItemStack>();large.addDrops(999999,blockAdditional,level,position);
        check(blockAdditional.size()==1001 && blockAdditional.stream().allMatch(s->s.count()==16),"block_additional_1001_stacks");
        small.addDrops(65,blockAdditional,level,position);
        check(blockAdditional.size()==1001,"subsequent_key_shares_provider_list");
        var wrench=new ArrayList<ItemStack>();wrench.add(new ItemStack("provider",1));large.addDrops(999999,wrench,level,position);
        check(wrench.size()==1001 && wrench.stream().skip(1).allMatch(s->s.count()==16),"wrench_part_consumes_one_slot");
        var cpuSecondKey=new ArrayList<ItemStack>();small.addDrops(65,cpuSecondKey,level,position);
        check(cpuSecondKey.size()==9 && cpuSecondKey.stream().mapToInt(ItemStack::count).sum()==65 && cpuSecondKey.stream().allMatch(s->s.count()<=8),"cpu_fresh_list_for_each_key");
    }
}
