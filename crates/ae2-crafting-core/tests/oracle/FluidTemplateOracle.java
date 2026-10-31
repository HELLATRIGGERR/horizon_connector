/* Test-only stubs for the actual pinned AECraftingPattern method.
 * The reference method is supplied by run-fluid-template-oracle.ts and is
 * Copyright (c) AE2 contributors, LGPL-3.0-or-later. */
import java.util.*;

class AEKey {}
class FluidKey extends AEKey {}
class Item {}
class BucketItem extends Item {}
class MilkBucketItem extends Item {}
class Items { static final Item BUCKET = new Item(); }
record GenericStack(AEKey what,long amount) {}
class ItemStack {
    final Item item; final int count; final GenericStack fluid;
    ItemStack(Item item,int count,GenericStack fluid){this.item=item;this.count=count;this.fluid=fluid;}
    int getCount(){return count;}
    boolean is(Item candidate){return item==candidate;}
}
class AEItemKey extends AEKey {
    final ItemStack stack;
    AEItemKey(ItemStack stack){this.stack=stack;}
    Item getItem(){return stack.item;}
    ItemStack toStack(){return stack;}
}
class AEKeyType { static Object fluids(){return new Object();} }
class ContainerItemStrategies {
    static GenericStack getContainedStack(ItemStack item,Object type){return item.fluid;}
}
record Input(int width) {}
record Positioned(Input input,int left,int top) {}
class Recipe {
    List<ItemStack> remainders;
    Recipe(List<ItemStack> remainders){this.remainders=remainders;}
    List<ItemStack> getRemainingItems(Input input){return remainders;}
}
class SourcePattern {
    boolean canSubstituteFluids;
    Recipe recipe;
    Positioned positioned = new Positioned(new Input(3),0,0);
    Positioned makeCraftingInput(){return positioned;}
    // ACTUAL_AE2_METHOD
    GenericStack call(int slot,GenericStack item){return getItemOrFluidInput(slot,item);}
}
public class FluidTemplateOracle {
    static void check(String label,boolean ok){if(!ok)throw new AssertionError(label);System.out.println(label+"=1");}
    public static void main(String[] args) {
        var fluid=new FluidKey();var contained=new GenericStack(fluid,1000);
        var filled=new AEItemKey(new ItemStack(new BucketItem(),1,contained));var item=new GenericStack(filled,1);
        var source=new SourcePattern();source.recipe=new Recipe(List.of(new ItemStack(Items.BUCKET,1,null)));
        source.canSubstituteFluids=false;check("disabled_keeps_item",source.call(0,item)==item);
        source.canSubstituteFluids=true;var actual=source.call(0,item);
        check("eligible_bucket_uses_fluid",actual.what()==fluid && actual.amount()==1000);
        source.recipe=new Recipe(List.of(new ItemStack(new Item(),1,null)));
        check("wrong_remainder_keeps_item",source.call(0,item)==item);
        source.recipe=new Recipe(List.of(new ItemStack(Items.BUCKET,2,null)));
        check("multiple_remainders_keep_item",source.call(0,item)==item);
        source.recipe=new Recipe(List.of(new ItemStack(Items.BUCKET,1,null)));
        var nonBucket=new GenericStack(new AEItemKey(new ItemStack(new Item(),1,contained)),1);
        check("non_bucket_keeps_item",source.call(0,nonBucket)==nonBucket);
        var emptyBucket=new GenericStack(new AEItemKey(new ItemStack(new BucketItem(),1,null)),1);
        check("no_contained_fluid_keeps_item",source.call(0,emptyBucket)==emptyBucket);
        var milk=new GenericStack(new AEItemKey(new ItemStack(new MilkBucketItem(),1,contained)),1);
        check("fluid_capable_milk_bucket_allowed",source.call(0,milk).what()==fluid);
    }
}
