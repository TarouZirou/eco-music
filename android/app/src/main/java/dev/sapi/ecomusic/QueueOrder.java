package dev.sapi.ecomusic;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Random;

public final class QueueOrder {
    public static <T> List<T> shuffled(List<T> input, int selected, Random random) {
        List<T> result = new ArrayList<>(input);
        if (selected >= input.size() || selected < -1) throw new IllegalArgumentException("選択範囲外");
        if (selected >= 0) {
            T first = result.remove(selected);
            Collections.shuffle(result, random);
            result.add(0, first);
        } else Collections.shuffle(result, random);
        return result;
    }
}
