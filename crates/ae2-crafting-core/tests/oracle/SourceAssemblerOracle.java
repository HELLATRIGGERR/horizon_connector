/* Test-only stubs around methods extracted from AE2 19.2.18 (LGPL-3.0-or-later).
 * These model the assembler's item/fluid table operations, not a Minecraft server.
 */
import java.util.*;
interface AEKey {}
record AEItemKey(String id) implements AEKey { ItemStack toStack(){return new ItemStack(id,1);} }
record AEFluidKey(String id) implements AEKey {}
record GenericStack(AEKey what,long amount) {
    static ItemStack wrapInItemStack(AEKey what,int amount){return new ItemStack("wrapped:"+what,amount);}
}
record ItemStack(String id,int count) {boolean isEmpty(){return count==0;}}
enum Direction { NORTH }
class KeyCounter implements Iterable<KeyCounter.Entry> {
    record Entry(AEKey key,long amount) {AEKey getKey(){return key;} long getLongValue(){return amount;}}
    final LinkedHashMap<AEKey,Long> values=new LinkedHashMap<>();
    void add(AEKey key,long amount){values.merge(key,amount,Long::sum);}
    long get(AEKey key){return values.getOrDefault(key,0L);}
    void remove(AEKey key,long amount){values.put(key,get(key)-amount);}
    void removeZeros(){values.values().removeIf(n->n==0);}
    boolean isEmpty(){return values.isEmpty();}
    public Iterator<Entry> iterator(){return values.entrySet().stream().map(e->new Entry(e.getKey(),e.getValue())).toList().iterator();}
}
interface IPatternDetails {
    // ACTUAL_GENERIC_CAPABILITY
}
interface IMolecularAssemblerSupportedPattern extends IPatternDetails {
    void fillCraftingGrid(KeyCounter[] table,CraftingGridAccessor gridAccessor);
    // ACTUAL_ASSEMBLER_CAPABILITY
    interface CraftingGridAccessor {void set(int slot,ItemStack stack);}
}
class AECraftingPattern implements IPatternDetails,IMolecularAssemblerSupportedPattern {
    final int[] compressed=new int[9];
    final GenericStack[] fluids=new GenericStack[9];
    AECraftingPattern(){Arrays.fill(compressed,-1);}
    int getCompressedIndexFromSparse(int sparse){return compressed[sparse];}
    GenericStack getValidFluid(int slot){return fluids[slot];}
    // ACTUAL_FILL_CRAFTING_GRID
}
class Inventory {
    final ItemStack[] slots=new ItemStack[9];
    boolean isEmpty(){return Arrays.stream(slots).allMatch(Objects::isNull);}
    void setItemDirect(int i,ItemStack stack){slots[i]=stack;}
}
interface CraftingMachine {boolean pushPattern(IPatternDetails details,KeyCounter[] table,Direction where);}
class MolecularAssemblerBlockEntity implements CraftingMachine {
    final ItemStack myPattern=new ItemStack("empty",0);
    final Inventory gridInv=new Inventory();
    final Inventory patternInv=new Inventory();
    boolean forcePlan,saved;
    IMolecularAssemblerSupportedPattern myPlan;
    Direction pushDirection;
    void updateSleepiness(){}
    void saveChanges(){saved=true;}
    // ACTUAL_FILL_GRID
    // ACTUAL_PUSH_PATTERN
}
public class SourceAssemblerOracle {
    static void check(boolean value,String name){if(!value)throw new AssertionError(name);System.out.println(name+"=1");}
    static KeyCounter counter(AEKey key,long amount){var result=new KeyCounter();result.add(key,amount);return result;}
    static void leftover(AECraftingPattern pattern,AEKey key,long amount,long left,String name){
        var assembler=new MolecularAssemblerBlockEntity();var table=counter(key,amount);
        try {assembler.pushPattern(pattern,new KeyCounter[]{table},Direction.NORTH);throw new AssertionError(name+" accepted");}
        catch(RuntimeException expected){
            check(expected.getMessage().startsWith("Could not fill grid with some items"),name+"_throws");
            check(table.get(key)==left,name+"_left_in_table");
            check(!assembler.gridInv.isEmpty() && assembler.forcePlan && assembler.myPlan==pattern && !assembler.saved,name+"_partial_mutation");
        }
    }
    public static void main(String[] args){
        var item=new AEItemKey("oak");var fluid=new AEFluidKey("water");
        var singular=new AECraftingPattern();singular.compressed[0]=0;
        check(!singular.supportsPushInputsToExternalInventory(),"assembler_pattern_rejects_generic_targets");
        var normal=new MolecularAssemblerBlockEntity();var one=counter(item,1);
        check(normal.pushPattern(singular,new KeyCounter[]{one},Direction.NORTH) && one.isEmpty() && normal.saved,"singular_is_consumed_and_saved");
        leftover(singular,item,4,3,"one_slot_amount_two_primary");
        leftover(singular,item,2,1,"one_slot_amount_two_substitute");
        var twoSlots=new AECraftingPattern();twoSlots.compressed[0]=0;twoSlots.compressed[1]=0;
        leftover(twoSlots,item,6,4,"two_slots_amount_two_and_one_primary");
        var bucket=new AECraftingPattern();bucket.compressed[0]=0;bucket.fluids[0]=new GenericStack(fluid,1000);
        leftover(bucket,fluid,2000,1000,"nonunit_bucket_fluid_primary");
    }
}
