/* Test-only infrastructure for unchanged pinned AE2 PathingCalculation.
 * Adapted AE2 method bodies are appended by run-pathing-oracle.ts.
 * LGPL-3.0-or-later; copyright AlgorithmX2 and AE2 contributors. */
import java.util.*;
class Logger { void error(String text, Object... args) {} }
class LoggerFactory { static Logger getLogger(Class<?> type) { return new Logger(); } }
class Reference2IntOpenHashMap<T> extends IdentityHashMap<T,Integer> {
    int addTo(T key, int delta) { int old=getOrDefault(key,0); put(key,old+delta); return old; }
}
class ImmutableList {
    static <T> List<T> copyOf(Collection<? extends T> values) { return List.copyOf(values); }
    @SafeVarargs static <T> List<T> of(T... values) { return List.of(values); }
}
enum GridFlags { DENSE_CAPACITY, PREFERRED, REQUIRE_CHANNEL, COMPRESSED_CHANNEL, CANNOT_CARRY_COMPRESSED, MULTIBLOCK, CANNOT_CARRY }
enum ChannelMode { DEFAULT, INFINITE; int getCableCapacityFactor() { return 1; } }
class PathingService { ChannelMode getChannelMode() { return ChannelMode.DEFAULT; } }
interface IGrid { List<GridNode> getMachineNodes(Class<?> type); PathingService getPathingService(); }
interface IGridNode { <T> T getService(Class<T> type); }
interface IGridMultiblock { Iterator<IGridNode> getMultiblockNodes(); }
class ControllerBlockEntity {}
class Direction { Direction getOpposite() { return this; } }
class GridNode implements IGridNode,IPathItem {
    static final Logger LOG=new Logger();
    final int id; final Set<GridFlags> flags=EnumSet.noneOf(GridFlags.class);
    final List<GridConnection> connections=new ArrayList<>();
    IGrid myGrid; Object owner=new Object(); List<IGridNode> multiblock;
    int usedChannels,subtreeMaxChannels; GridNode highestSimilarAncestor;
    boolean subtreeAllowsCompressedChannels;
    GridNode(int id,int type) {
        this.id=id;
        if(type==0) {owner=new ControllerBlockEntity();flags.add(GridFlags.DENSE_CAPACITY);}
        if(type==1)flags.add(GridFlags.DENSE_CAPACITY);
        if(type==1||type==2)flags.add(GridFlags.PREFERRED);
        if(type>=3)flags.add(GridFlags.REQUIRE_CHANNEL);
    }
    Object getOwner(){return owner;} IGrid getGrid(){return myGrid;}
    List<GridConnection> getConnections(){return connections;}
    public boolean hasFlag(GridFlags flag){return flags.contains(flag);}
    public IPathItem getControllerRoute(){return connections.getFirst();}
    GridNode getHighestSimilarAncestor(){return highestSimilarAncestor;}
    boolean getSubtreeAllowsCompressedChannels(){return subtreeAllowsCompressedChannels;}
    @SuppressWarnings("unchecked") public <T>T getService(Class<T> type){
        if(type==IGridMultiblock.class&&multiblock!=null)return (T)(IGridMultiblock)(()->multiblock.iterator());
        return null;
    }
    public void setAdHocChannels(int count){usedChannels=count;}
    public void finalizeChannels(){}
    /* NODE_METHODS */
}
class GridConnection implements IPathItem {
    GridNode sideA,sideB; Direction fromAtoB; int usedChannels;
    GridConnection(GridNode a,GridNode b){sideA=a;sideB=b;a.connections.add(this);b.connections.add(this);}
    GridNode a(){return sideA;} GridNode b(){return sideB;}
    GridNode getOtherSide(GridNode node){return node==sideA?sideB:sideA;}
    public void setAdHocChannels(int count){usedChannels=count;}
    public void finalizeChannels(){}
    /* EDGE_METHODS */
}
class Grid implements IGrid {
    final List<GridNode> nodes; Grid(List<GridNode> nodes){this.nodes=nodes;for(var node:nodes)node.myGrid=this;}
    public List<GridNode>getMachineNodes(Class<?>type){return nodes.stream().filter(node->node.owner instanceof ControllerBlockEntity).toList();}
    public PathingService getPathingService(){return new PathingService();}
}
public class PathingOracle {
    public static void main(String[]args) {
        Scanner input=new Scanner(System.in);int cases=input.nextInt();StringJoiner results=new StringJoiner(",","[","]");
        for(int c=0;c<cases;c++){
            int n=input.nextInt(),m=input.nextInt();List<GridNode>nodes=new ArrayList<>();int[]groups=new int[n];
            for(int i=0;i<n;i++){int type=input.nextInt();groups[i]=input.nextInt();nodes.add(new GridNode(i,type));}
            for(int i=0;i<n;i++)if(groups[i]>=0){int group=groups[i];List<IGridNode>members=new ArrayList<>();for(int j=0;j<n;j++)if(groups[j]==group)members.add(nodes.get(j));nodes.get(i).multiblock=members;nodes.get(i).flags.add(GridFlags.MULTIBLOCK);}
            Grid grid=new Grid(nodes);
            for(int i=0;i<m;i++)new GridConnection(nodes.get(input.nextInt()),nodes.get(input.nextInt()));
            PathingCalculation calculation=new PathingCalculation(grid);calculation.compute();
            StringJoiner used=new StringJoiner(",","[","]"),assigned=new StringJoiner(",","[","]");
            for(var node:nodes){used.add(Integer.toString(node.usedChannels));assigned.add(Boolean.toString(!node.hasFlag(GridFlags.REQUIRE_CHANNEL)||node.usedChannels>0));}
            results.add("{\"channels\":"+calculation.getChannelsInUse()+",\"blocks\":"+calculation.getChannelsByBlocks()+",\"used\":"+used+",\"assigned\":"+assigned+"}");
        }
        System.out.println(results);
    }
}
