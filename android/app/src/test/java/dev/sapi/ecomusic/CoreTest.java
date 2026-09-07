package dev.sapi.ecomusic;
import org.junit.Test;
import static org.junit.Assert.*;
import java.util.*;

public class CoreTest {
    @Test public void canonicalPlaylist() {
        assertEquals("https://www.youtube.com/playlist?list=PLabc_12", Urls.playlist("https://music.youtube.com/watch?v=abcdefghijk&list=PLabc_12"));
    }
    @Test public void rejectForeignOrigins() {
        for (String s : List.of("http://youtube.com/?list=PL123", "https://youtube.com.evil.test/?list=PL123", "file:///tmp/a", "https://x@youtube.com/?list=PL123", "https://youtube.com/?list=PL1&list=PL2", "https://youtube.com/?list=RD123")) {
            assertThrows(IllegalArgumentException.class, () -> Urls.playlist(s));
        }
    }
    @Test public void validatesTrack() {
        assertEquals("https://www.youtube.com/watch?v=abcdefghijk", Urls.track("https://music.youtube.com/watch?v=abcdefghijk"));
        assertThrows(IllegalArgumentException.class, () -> Urls.track("https://youtube.com/watch?v=invalid"));
    }
    @Test public void shuffleVisitsEveryEntryOnce() {
        List<Integer> source = new ArrayList<>(); for(int i=0;i<2000;i++) source.add(i);
        for(int seed=0;seed<20;seed++) {
            List<Integer> out=QueueOrder.shuffled(source, -1, new Random(seed));
            assertNotEquals(source,out);
            Collections.sort(out); assertEquals(source,out);
        }
    }
    @Test public void selectedFirstPreservesDuplicateOccurrences() {
        List<String> input=List.of("A","B","A","C");
        List<String> result=QueueOrder.shuffled(input,2,new Random(5));
        assertEquals("A",result.get(0));
        assertEquals(2,Collections.frequency(result,"A"));
        assertEquals(4,result.size()); assertEquals(List.of("A","B","A","C"),input);
    }
}
