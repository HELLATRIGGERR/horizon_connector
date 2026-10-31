import it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap;
public class Ae2FastutilOrder {
  static long key(int i) { int x=i*17-300,z=i*31-200; return ((long)z << 32) | (x & 0xffffffffL); }
  static void dump(Long2ObjectOpenHashMap<String> m) { System.out.println(java.util.Arrays.toString(m.keySet().toLongArray())); }
  public static void main(String[] args) { var m=new Long2ObjectOpenHashMap<String>();m.put(0L,"0");for(int i=0;i<70;i++)m.put(key(i),Integer.toString(i));dump(m);for(int i=0;i<70;i++)if(i%3!=0)m.remove(key(i));dump(m);for(int i=70;i<115;i++)m.put(key(i),Integer.toString(i));m.remove(0L);dump(m); }
}
