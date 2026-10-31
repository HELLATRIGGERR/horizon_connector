/* Test-only stubs around AECraftingPattern methods extracted from AE2
 * 19.2.18 (LGPL-3.0-or-later). They model positioned crafting inputs and
 * an overlapping shapeless recipe, not the Minecraft recipe registry.
 */
import java.util.*;

record ItemStack(String id) {
    static final ItemStack EMPTY=new ItemStack("");
    ItemStack copy(){return this;}
    static boolean matches(ItemStack a,ItemStack b){return Objects.equals(a,b);}
}
record AEItemKey(String id) {
    static AEItemKey of(ItemStack item){return item.id().isEmpty()?null:new AEItemKey(item.id());}
    ItemStack toStack(){return new ItemStack(id);}
}
record GenericStack(AEItemKey what,long amount) {
    static GenericStack unwrapItemStack(ItemStack item){return null;}
}
class NonNullList<T> extends ArrayList<T> {
    static <T> NonNullList<T> withSize(int size,T value){
        var result=new NonNullList<T>();for(int i=0;i<size;i++)result.add(value);return result;
    }
}
class Level { Object registryAccess(){return new Object();} }
class CraftingInput {
    final int width,height;final List<ItemStack> items;
    CraftingInput(int width,int height,List<ItemStack> items){this.width=width;this.height=height;this.items=items;}
    int width(){return width;}int height(){return height;}int size(){return width*height;}
    ItemStack getItem(int i){return items.get(i);}ItemStack getItem(int x,int y){return items.get(x+y*width);}
    static CraftingInput of(int width,int height,List<ItemStack> items){return positioned(width,height,items).input();}
    static Positioned positioned(int width,int height,List<ItemStack> items){
        int left=width,top=height,right=-1,bottom=-1;
        for(int i=0;i<items.size();i++)if(!items.get(i).id().isEmpty()){
            int x=i%width,y=i/width;left=Math.min(left,x);top=Math.min(top,y);right=Math.max(right,x);bottom=Math.max(bottom,y);
        }
        if(right<0)return new Positioned(new CraftingInput(0,0,List.of()),0,0);
        int w=right-left+1,h=bottom-top+1;var result=new ArrayList<ItemStack>();
        for(int y=top;y<=bottom;y++)for(int x=left;x<=right;x++)result.add(items.get(x+y*width));
        return new Positioned(new CraftingInput(w,h,result),left,top);
    }
    record Positioned(CraftingInput input,int left,int top) {}
}
class Recipe {
    final boolean special;
    Recipe(boolean special){this.special=special;}
    boolean isSpecial(){return special;}
    boolean matches(CraftingInput input,Level level){
        if(input.size()!=2)return false;
        var a=input.getItem(0).id();var b=input.getItem(1).id();
        return (a.equals("stone") || a.equals("andesite")) && (b.equals("stone") || b.equals("andesite"))
                && (a.equals("stone") || b.equals("stone"));
    }
    ItemStack assemble(CraftingInput input,Object registry){
        if(input.size()!=2)return ItemStack.EMPTY;
        if(special)return new ItemStack(input.getItem(0).id().equals("andesite") ? "dynamic" : "diamond");
        if(!matches(input,new Level()))return ItemStack.EMPTY;
        return new ItemStack("diamond");
    }
}
class AECraftingPattern {
    static final int CRAFTING_GRID_DIMENSION=3;
    final List<GenericStack> sparseInputs=new ArrayList<>();
    final CraftingInput.Positioned positionedPattern;
    final Recipe recipe;final boolean canSubstitute;
    final ItemStack output=new ItemStack("diamond");
    AECraftingPattern(boolean special,boolean substitutes){
        recipe=new Recipe(special);canSubstitute=substitutes;
        for(int i=0;i<9;i++)sparseInputs.add(i<2?new GenericStack(new AEItemKey("stone"),1):null);
        positionedPattern=makeInput(-1,null);
    }
    CraftingInput.Positioned makeInput(int replaced,AEItemKey key){
        var items=new ArrayList<ItemStack>();
        for(int i=0;i<9;i++){
            var value=i==replaced?key:(sparseInputs.get(i)==null?null:sparseInputs.get(i).what());
            items.add(value==null?ItemStack.EMPTY:value.toStack());
        }
        return CraftingInput.positioned(3,3,items);
    }
    CraftingInput.Positioned makeCraftingInputWithReplacedSlot(int slot,AEItemKey key){return makeInput(slot,key);}
    Boolean getTestResult(int slot,AEItemKey key){return null;}
    void setTestResult(int slot,AEItemKey key,boolean result){}
    GenericStack getValidFluid(int slot){return null;}
    // ACTUAL_IS_ITEM_VALID
    // ACTUAL_ASSEMBLE
}
public class SourcePatternAssemblyOracle {
    static void check(boolean value,String name){if(!value)throw new AssertionError(name);System.out.println(name+"=1");}
    static CraftingInput positioned(String a,String b,int row){
        var slots=new ArrayList<>(Collections.nCopies(9,ItemStack.EMPTY));
        slots.set(row*3,new ItemStack(a));slots.set(row*3+1,new ItemStack(b));
        return CraftingInput.of(3,3,slots);
    }
    public static void main(String[] args){
        var level=new Level();var pattern=new AECraftingPattern(false,true);
        check(pattern.isItemValid(0,new AEItemKey("andesite"),level) && pattern.isItemValid(1,new AEItemKey("andesite"),level),"independent_substitutions_match_original_output");
        var both=positioned("andesite","andesite",0);
        check(!pattern.recipe.matches(both,level),"combined_grid_does_not_match_recipe");
        check(pattern.assemble(both,level).equals(new ItemStack("diamond")),"ordinary_pattern_returns_original_output");
        check(pattern.assemble(positioned("andesite","stone",2),level).equals(new ItemStack("diamond")),"shifted_frame_preserves_relative_slots");
        var strict=new AECraftingPattern(false,false);
        check(strict.assemble(positioned("andesite","stone",0),level).equals(ItemStack.EMPTY),"disabled_substitution_rejects_replacement");
        var dynamic=new AECraftingPattern(true,true);
        check(!dynamic.recipe.matches(both,level),"special_recipe_grid_does_not_match");
        check(dynamic.assemble(both,level).equals(new ItemStack("dynamic")),"special_recipe_assembles_without_matching");
    }
}
