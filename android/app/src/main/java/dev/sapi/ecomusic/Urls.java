package dev.sapi.ecomusic;

import java.net.URI;
import java.net.URLDecoder;

import java.util.Set;

public final class Urls {
    private static final Set<String> HOSTS = Set.of("music.youtube.com", "youtube.com", "www.youtube.com", "m.youtube.com");
    private static String decode(String value) {
        try { return URLDecoder.decode(value, "UTF-8"); }
        catch (java.io.UnsupportedEncodingException e) { throw new AssertionError(e); }
    }
    private static String id(String value, String key) {
        URI u = URI.create(value.trim());
        if (!"https".equals(u.getScheme()) || !HOSTS.contains(u.getHost()) || u.getUserInfo() != null || (u.getPort() != -1 && u.getPort() != 443))
            throw new IllegalArgumentException("YouTube MusicのHTTPS URLを入力してください");
        String result = null;
        for (String pair : (u.getRawQuery() == null ? "" : u.getRawQuery()).split("&")) {
            String[] kv = pair.split("=", 2);
            if (kv.length == 2 && key.equals(decode(kv[0]))) {
                if (result != null) throw new IllegalArgumentException("IDが重複しています");
                result = decode(kv[1]);
            }
        }
        if (result == null || !result.matches("[A-Za-z0-9_-]{2,200}")) throw new IllegalArgumentException("再生リストURLを確認してください");
        return result;
    }
    public static String playlist(String value) {
        String id = id(value, "list");
        if (id.startsWith("RD") || id.startsWith("UL")) throw new IllegalArgumentException("ラジオではなく固定の再生リストを選んでください");
        return "https://www.youtube.com/playlist?list=" + id;
    }
    public static String track(String value) {
        String id = id(value, "v");
        if (id.length() != 11) throw new IllegalArgumentException("不正な動画ID");
        return "https://www.youtube.com/watch?v=" + id;
    }
}
