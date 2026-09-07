package dev.sapi.ecomusic;

import android.app.PendingIntent;
import android.content.Intent;
import android.os.Handler;
import android.os.Looper;
import androidx.media3.common.AudioAttributes;
import androidx.media3.common.C;
import androidx.media3.common.MediaItem;
import androidx.media3.common.MimeTypes;
import androidx.media3.common.PlaybackException;
import androidx.media3.common.Player;
import androidx.media3.datasource.DefaultHttpDataSource;
import androidx.media3.datasource.ResolvingDataSource;
import androidx.media3.exoplayer.DefaultLoadControl;
import androidx.media3.exoplayer.ExoPlayer;
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory;
import androidx.media3.session.MediaSession;
import androidx.media3.session.MediaSessionService;
import com.google.common.util.concurrent.Futures;
import com.google.common.util.concurrent.ListenableFuture;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;

@androidx.annotation.OptIn(markerClass = androidx.media3.common.util.UnstableApi.class)
public final class PlaybackService extends MediaSessionService {
    private ExoPlayer player;
    private MediaSession session;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private Future<?> prefetch;
    private int retries = 0, generation = 0;

    @Override public void onCreate() {
        super.onCreate();
        var http = new DefaultHttpDataSource.Factory()
            .setUserAgent("Mozilla/5.0 EcoMusic/0.1")
            .setConnectTimeoutMs(15000).setReadTimeoutMs(20000);
        var resolving = new ResolvingDataSource.Factory(http, spec -> spec.withUri(Extractor.resolve(spec.uri.toString())));
        var buffers = new DefaultLoadControl.Builder()
            .setBufferDurationsMs(15000, 90000, 1500, 5000)
            .setTargetBufferBytes(8 * 1024 * 1024)
            .setPrioritizeTimeOverSizeThresholds(false)
            .setBackBuffer(0, false).build();
        player = new ExoPlayer.Builder(this)
            .setMediaSourceFactory(new DefaultMediaSourceFactory(resolving))
            .setLoadControl(buffers).build();
        player.setAudioAttributes(new AudioAttributes.Builder()
            .setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(), true);
        player.setHandleAudioBecomingNoisy(true);
        player.setWakeMode(C.WAKE_MODE_NETWORK);
        player.addListener(new Player.Listener() {
            @Override public void onMediaItemTransition(MediaItem item, int reason) {
                generation++; retries = 0;
                warmNext();
            }
            @Override public void onPlayerError(PlaybackException error) {
                final int token = ++generation;
                final int index = player.getCurrentMediaItemIndex();
                final long position = player.getCurrentPosition();
                if (++retries > 3) { player.pause(); return; }
                long delay = (1L << retries) * 1000L;
                handler.postDelayed(() -> {
                    if (token != generation || !player.getPlayWhenReady()) return;
                    Extractor.invalidate();
                    player.seekTo(index, position);
                    player.prepare();
                }, delay);
            }
            @Override public void onPlayWhenReadyChanged(boolean ready, int reason) {
                if (!ready) generation++;
            }
            @Override public void onPlaybackStateChanged(int state) {
                if (state == Player.STATE_ENDED) player.pause();
            }
        });
        PendingIntent activity = PendingIntent.getActivity(this, 0,
            new Intent(this, MainActivity.class), PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        session = new MediaSession.Builder(this, player).setSessionActivity(activity)
            .setCallback(new MediaSession.Callback() {
                @Override public MediaSession.ConnectionResult onConnect(MediaSession s, MediaSession.ControllerInfo c) {
                    if (!getPackageName().equals(c.getPackageName()) && !c.isTrusted())
                        return MediaSession.ConnectionResult.reject();
                    return MediaSession.Callback.super.onConnect(s, c);
                }
                @Override public ListenableFuture<List<MediaItem>> onAddMediaItems(MediaSession s,
                        MediaSession.ControllerInfo c, List<MediaItem> items) {
                    List<MediaItem> safe = new ArrayList<>();
                    try {
                        for (MediaItem item : items) {
                            String url = Urls.track(item.mediaId);
                            safe.add(new MediaItem.Builder().setMediaId(url).setUri(url)
                                .setMimeType(MimeTypes.AUDIO_UNKNOWN).setMediaMetadata(item.mediaMetadata).build());
                        }
                    } catch (RuntimeException e) { return Futures.immediateFailedFuture(e); }
                    return Futures.immediateFuture(safe);
                }
            }).build();
    }
    private void warmNext() {
        if (prefetch != null) prefetch.cancel(true);
        int next = player.getNextMediaItemIndex();
        if (next == C.INDEX_UNSET) return;
        String url = player.getMediaItemAt(next).mediaId;
        prefetch = worker.submit(() -> {
            try { Extractor.resolve(url); } catch (Exception ignored) { /* Playback retries independently. */ }
        });
    }
    @Override public MediaSession onGetSession(MediaSession.ControllerInfo c) { return session; }
    @Override public void onTaskRemoved(Intent rootIntent) {
        if (!player.getPlayWhenReady() || player.getMediaItemCount() == 0 || player.getPlaybackState() == Player.STATE_ENDED) stopSelf();
    }
    @Override public void onDestroy() {
        generation++; handler.removeCallbacksAndMessages(null); worker.shutdownNow();
        if (session != null) session.release();
        if (player != null) player.release();
        super.onDestroy();
    }
}
