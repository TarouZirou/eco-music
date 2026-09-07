package dev.sapi.ecomusic;

import android.net.Uri;
import android.os.SystemClock;
import java.io.IOException;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.schabi.newpipe.extractor.Page;
import org.schabi.newpipe.extractor.ServiceList;
import org.schabi.newpipe.extractor.playlist.PlaylistInfo;
import org.schabi.newpipe.extractor.stream.AudioStream;
import org.schabi.newpipe.extractor.stream.DeliveryMethod;
import org.schabi.newpipe.extractor.stream.StreamInfo;
import org.schabi.newpipe.extractor.stream.StreamInfoItem;

public final class Extractor {
    public final static class Track {
        public final String title, url;
        public Track(String t, String u) { title=t; url=u; }
        @Override public String toString() { return title; }
    }
    public static final class Playlist {
        public final String title;
        public final List<Track> tracks;
        Playlist(String t, List<Track> list) { title=t; tracks=list; }
    }
    // Store only three short-lived URLs, never full StreamInfo trees or audio files.
    private static final Map<String, Entry> CACHE = new LinkedHashMap<String, Entry>(4, .75f, true) {
        @Override protected boolean removeEldestEntry(Map.Entry<String, Extractor.Entry> e) { return size() > 3; }
    };
    private static final class Entry {
        final Uri uri; final long created = SystemClock.elapsedRealtime();
        Entry(Uri value) { uri=value; }
    }
    public static Playlist playlist(String value) throws Exception {
        String url = Urls.playlist(value);
        PlaylistInfo info = PlaylistInfo.getInfo(ServiceList.YouTube, url);
        List<Track> tracks = new ArrayList<>();
        append(tracks, info.getRelatedItems());
        Page page = info.getNextPage();
        while (Page.isValid(page)) {
            if (Thread.currentThread().isInterrupted()) throw new InterruptedException();
            var more = PlaylistInfo.getMoreItems(ServiceList.YouTube, url, page);
            append(tracks, more.getItems()); page = more.getNextPage();
        }
        if (tracks.isEmpty()) throw new IOException("再生できる曲がありません。公開設定を確認してください。");
        return new Playlist(info.getName(), tracks);
    }
    private static void append(List<Track> out, List<StreamInfoItem> items) {
        for (StreamInfoItem item : items) {
            try { out.add(new Track(item.getName(), Urls.track(item.getUrl()))); }
            catch (IllegalArgumentException ignored) { }
        }
    }
    public static void invalidate() { synchronized (CACHE) { CACHE.clear(); } }
    public static Uri resolve(String value) throws IOException {
        String url = Urls.track(value);
        Entry entry;
        synchronized (CACHE) { entry = CACHE.get(url); }
        if (entry != null && SystemClock.elapsedRealtime() - entry.created < 10 * 60_000L) return entry.uri;
        try {
            AudioStream best = null;
            int bestScore = Integer.MIN_VALUE;
            for (AudioStream a : StreamInfo.getInfo(ServiceList.YouTube, url).getAudioStreams()) {
                if (!a.isUrl() || a.getDeliveryMethod() != DeliveryMethod.PROGRESSIVE_HTTP) continue;
                int rate = a.getAverageBitrate();
                int score = rate > 0 && rate <= 128 ? 10000 + rate : (rate > 128 ? -rate : -10000);
                if (score > bestScore) { best=a; bestScore=score; }
            }
            if (best == null) throw new IOException("対応する音声ストリームがありません");
            Uri uri = Uri.parse(best.getContent());
            if (!"https".equals(uri.getScheme())) throw new IOException("安全な音声URLを取得できません");
            synchronized (CACHE) { CACHE.put(url, new Entry(uri)); } return uri;
        } catch (Exception e) {
            throw new IOException("音声取得失敗。通信またはExtractorの更新を確認してください。", e);
        }
    }
}
