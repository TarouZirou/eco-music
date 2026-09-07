package dev.sapi.ecomusic;

import android.Manifest;
import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.WindowInsets;
import android.widget.*;
import androidx.media3.common.MediaItem;
import androidx.media3.common.MediaMetadata;
import androidx.media3.common.PlaybackException;
import androidx.media3.common.Player;
import androidx.media3.session.MediaController;
import androidx.media3.session.SessionToken;
import com.google.common.util.concurrent.ListenableFuture;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import org.json.JSONArray;
import org.json.JSONObject;

public final class MainActivity extends Activity {
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private ListenableFuture<MediaController> future;
    private MediaController controller;
    private EditText url;
    private TextView status, now;
    private Spinner savedView;
    private CheckBox shuffle, repeat;
    private Button loadButton, playButton;
    private ListView list;
    private final List<Extractor.Track> tracks = new ArrayList<>();
    private JSONArray bookmarks;
    private SharedPreferences prefs;
    private String loadedUrl = "", loadedTitle = "";
    private boolean busy = false;
    private final Player.Listener listener = new Player.Listener() {
        @Override public void onEvents(Player p, Player.Events events) { renderPlayer(); }
        @Override public void onPlayerError(PlaybackException e) { status.setText("再生エラー。2・4・8秒後に再接続します。失敗が続く場合は停止します。"); }
    };

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        prefs = getSharedPreferences("eco", MODE_PRIVATE);
        try { bookmarks = new JSONArray(prefs.getString("playlists", "[]")); }
        catch (Exception e) { bookmarks = new JSONArray(); }
        LinearLayout root = new LinearLayout(this); root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(dp(18), dp(18), dp(18), dp(12));
        setContentView(root);
        if (Build.VERSION.SDK_INT >= 30) root.setOnApplyWindowInsetsListener((v, insets) -> {
            var bars = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.ime());
            v.setPadding(dp(18) + bars.left, dp(18) + bars.top, dp(18) + bars.right, dp(12) + bars.bottom);
            return insets;
        });
        TextView heading = text("Eco Music", 28); root.addView(heading);
        root.addView(text("音声のみ · 公開 / 限定公開リスト", 13));
        savedView = new Spinner(this); root.addView(savedView); refreshBookmarks();
        savedView.setOnItemSelectedListener(new AdapterView.OnItemSelectedListener() {
            public void onNothingSelected(AdapterView<?> p) { }
            public void onItemSelected(AdapterView<?> p, View v, int pos, long id) {
                if (pos > 0) { JSONObject row=bookmarks.optJSONObject(pos - 1); if (row != null) url.setText(row.optString("url")); }
            }
        });
        url = new EditText(this); url.setSingleLine(true); url.setTextSize(14);
        url.setHint("YouTube Musicの再生リストURL"); root.addView(url);
        url.setText(prefs.getString("last_url", ""));
        LinearLayout actions = row(root);
        loadButton = button(actions, "取得", this::load);
        button(actions, "登録", this::saveBookmark);
        button(actions, "削除", this::removeBookmark);
        shuffle = new CheckBox(this); shuffle.setText("シャッフル（重複なし・次の再生開始時）"); shuffle.setChecked(prefs.getBoolean("shuffle", true)); root.addView(shuffle);
        shuffle.setOnCheckedChangeListener((b, checked) -> { prefs.edit().putBoolean("shuffle", checked).apply(); });
        repeat = new CheckBox(this); repeat.setText("全曲リピート"); repeat.setChecked(prefs.getBoolean("repeat", false)); root.addView(repeat);
        repeat.setOnCheckedChangeListener((b, checked) -> { prefs.edit().putBoolean("repeat", checked).apply(); if (controller != null) controller.setRepeatMode(checked ? Player.REPEAT_MODE_ALL : Player.REPEAT_MODE_OFF); });
        list = new ListView(this); list.setChoiceMode(ListView.CHOICE_MODE_SINGLE);
        root.addView(list, new LinearLayout.LayoutParams(-1, 0, 1));
        list.setOnItemClickListener((p,v,pos,id) -> play(pos));
        LinearLayout controls = row(root);
        playButton = button(controls, "再生", () -> play(-1)); playButton.setEnabled(false);
        button(controls, "一時停止 / 再開", () -> {
            if (controller == null) return;
            if (controller.getPlayWhenReady()) controller.pause();
            else { if (controller.getPlaybackState() == Player.STATE_IDLE) controller.prepare(); controller.play(); }
        });
        LinearLayout transport = row(root);
        button(transport, "前へ", () -> { if (controller != null) controller.seekToPreviousMediaItem(); });
        button(transport, "次へ", () -> { if (controller != null) controller.seekToNextMediaItem(); });
        button(transport, "停止", () -> { if (controller != null) { controller.pause(); controller.stop(); controller.clearMediaItems(); } });
        now = text("未再生", 16); now.setMaxLines(2); root.addView(now);
        status = text("URLを入力して「取得」を押してください", 12); status.setMaxLines(3); root.addView(status);
        acceptShare(getIntent());
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED)
            requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS}, 1);
    }
    @Override protected void onStart() {
        super.onStart();
        future = new MediaController.Builder(this, new SessionToken(this, new ComponentName(this, PlaybackService.class))).buildAsync();
        final ListenableFuture<MediaController> connecting = future;
        future.addListener(() -> {
            if (future != connecting || isDestroyed()) return;
            try {
                controller = connecting.get(); controller.addListener(listener);
                // Restore the service's existing playback state after screen rotation/reopening.
                if (controller.getMediaItemCount() > 0) {
                    repeat.setChecked(controller.getRepeatMode() == Player.REPEAT_MODE_ALL);
                }
                playButton.setEnabled(!tracks.isEmpty()); renderPlayer();
            } catch (Exception e) { status.setText("再生サービスへ接続できません"); }
        }, command -> runOnUiThread(command));
    }
    @Override protected void onStop() {
        if (controller != null) controller.removeListener(listener);
        if (future != null) MediaController.releaseFuture(future);
        future = null; controller = null; playButton.setEnabled(false);
        super.onStop();
    }
    @Override protected void onDestroy() { worker.shutdownNow(); super.onDestroy(); }
    @Override protected void onNewIntent(Intent intent) { super.onNewIntent(intent); setIntent(intent); acceptShare(intent); }
    private void acceptShare(Intent i) {
        if (!Intent.ACTION_SEND.equals(i.getAction())) return;
        String shared = i.getStringExtra(Intent.EXTRA_TEXT);
        if (shared == null) return;
        for (String s : shared.split("\\s+")) {
            try { url.setText(Urls.playlist(s)); return; } catch (Exception ignored) { }
        }
        status.setText("共有内容に対応する再生リストURLがありません");
    }
    private void load() {
        if (busy) return;
        final String target;
        try { target = Urls.playlist(url.getText().toString()); }
        catch (Exception e) { status.setText(e.getMessage()); return; }
        busy=true; loadButton.setEnabled(false); status.setText("リスト取得中… 全曲を取得します。再生中の音楽は継続します。");
        prefs.edit().putString("last_url", target).apply();
        worker.submit(() -> {
            try {
                Extractor.Playlist result = Extractor.playlist(target);
                runOnUiThread(() -> {
                    if (isDestroyed()) return;
                    tracks.clear(); tracks.addAll(result.tracks); loadedUrl=target; loadedTitle=result.title;
                    list.setAdapter(new ArrayAdapter<>(this, android.R.layout.simple_list_item_1, tracks));
                    status.setText(result.title + " · " + tracks.size() + "曲");
                    busy=false; loadButton.setEnabled(true); playButton.setEnabled(controller != null);
                });
            } catch (Exception e) {
                runOnUiThread(() -> { if (isDestroyed()) return; busy=false; loadButton.setEnabled(true);
                    status.setText("取得失敗。公開設定・通信を確認してください。改善しない場合はExtractorの更新が必要です。"); });
            }
        });
    }
    private void play(int selected) {
        if (controller == null || tracks.isEmpty()) return;
        List<MediaItem> queue = new ArrayList<>();
        for (Extractor.Track t : tracks) queue.add(new MediaItem.Builder().setMediaId(t.url).setUri(t.url)
            .setMediaMetadata(new MediaMetadata.Builder().setTitle(t.title).setArtist(loadedTitle).build()).build());
        int start = selected >= 0 ? selected : 0;
        controller.setShuffleModeEnabled(false);
        controller.setRepeatMode(repeat.isChecked() ? Player.REPEAT_MODE_ALL : Player.REPEAT_MODE_OFF);
        // Rotating the shuffle order around a random starting item can omit earlier items.
        // Shuffle the physical queue once and let playback traverse it from zero instead.
        if (shuffle.isChecked()) {
            queue = QueueOrder.shuffled(queue, selected, new java.util.Random());
            start=0; controller.setShuffleModeEnabled(false);
        }
        controller.setMediaItems(queue, start, 0); controller.prepare(); controller.play();
        status.setText("音声取得中…");
    }
    private void renderPlayer() {
        if (controller == null) return;
        CharSequence title=controller.getMediaMetadata().title;
        now.setText(title == null ? "未再生" : title);
        if (controller.getPlayerError() != null) {
            status.setText(controller.getPlayWhenReady() ? "再接続を試行中…" : "再接続に失敗して停止しました。次の曲か再開を選んでください。");
        } else if (controller.isPlaying()) status.setText("再生中 · 音声バッファ目標8MiB / 最大90秒");
        else if (controller.getPlaybackState() == Player.STATE_BUFFERING) status.setText("バッファ補充中…");
        else if (controller.getMediaItemCount() > 0) status.setText("一時停止 / 待機中");
    }
    private void refreshBookmarks() {
        List<String> names=new ArrayList<>(); names.add("登録済みリストを選択");
        for(int i=0;i<bookmarks.length();i++) names.add(bookmarks.optJSONObject(i).optString("title"));
        ArrayAdapter<String> adapter=new ArrayAdapter<>(this,android.R.layout.simple_spinner_item,names);
        adapter.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item); savedView.setAdapter(adapter);
    }
    private void saveBookmark() {
        try {
            if (loadedUrl.isEmpty() || !loadedUrl.equals(Urls.playlist(url.getText().toString()))) throw new IllegalArgumentException("先にこのリストを取得してください");
            JSONArray fresh=new JSONArray();
            for(int i=0;i<bookmarks.length();i++) if(!loadedUrl.equals(bookmarks.getJSONObject(i).getString("url"))) fresh.put(bookmarks.getJSONObject(i));
            if (fresh.length() >= 200) throw new IllegalArgumentException("登録は最大200件です");
            fresh.put(new JSONObject().put("title",loadedTitle).put("url",loadedUrl)); bookmarks=fresh;
            persist(); status.setText("再生リストを登録しました");
        } catch(Exception e) { status.setText(e.getMessage()); }
    }
    private void removeBookmark() {
        int index=savedView.getSelectedItemPosition()-1;
        if(index < 0) return;
        bookmarks.remove(index); persist();
    }
    private void persist() { prefs.edit().putString("playlists",bookmarks.toString()).apply(); refreshBookmarks(); }
    private int dp(int n) { return Math.round(n*getResources().getDisplayMetrics().density); }
    private TextView text(String s,int size) { TextView t=new TextView(this); t.setText(s); t.setTextSize(size); t.setPadding(0,dp(5),0,dp(5)); return t; }
    private LinearLayout row(LinearLayout root) { LinearLayout r=new LinearLayout(this); root.addView(r); return r; }
    private Button button(LinearLayout row,String title,Runnable action) { Button b=new Button(this); b.setText(title); b.setTextSize(13); b.setOnClickListener(v->action.run()); row.addView(b,new LinearLayout.LayoutParams(0,-2,1)); return b; }
}
