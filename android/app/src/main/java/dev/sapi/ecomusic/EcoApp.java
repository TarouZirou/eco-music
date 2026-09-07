package dev.sapi.ecomusic;

import android.app.Application;
import java.io.IOException;
import java.util.concurrent.TimeUnit;
import okhttp3.OkHttpClient;
import okhttp3.RequestBody;
import org.schabi.newpipe.extractor.NewPipe;
import org.schabi.newpipe.extractor.downloader.Downloader;
import org.schabi.newpipe.extractor.downloader.Request;
import org.schabi.newpipe.extractor.downloader.Response;

public final class EcoApp extends Application {
    @Override public void onCreate() {
        super.onCreate();
        NewPipe.init(new Downloader() {
            private final OkHttpClient client = new OkHttpClient.Builder()
                .connectTimeout(15, TimeUnit.SECONDS).readTimeout(20, TimeUnit.SECONDS)
                .callTimeout(40, TimeUnit.SECONDS).build();
            @Override public Response execute(Request r) throws IOException {
                okhttp3.Request.Builder b = new okhttp3.Request.Builder().url(r.url())
                    .header("User-Agent", "Mozilla/5.0 (Linux; Android 13) AppleWebKit/537.36 Chrome/131.0.0.0 Mobile Safari/537.36");
                r.headers().forEach((key, values) -> { b.removeHeader(key); for (String v : values) b.addHeader(key, v); });
                byte[] data = r.dataToSend();
                RequestBody body = data != null ? RequestBody.create(data, null) : null;
                if (body == null && (r.httpMethod().equals("POST") || r.httpMethod().equals("PUT"))) body = RequestBody.create(new byte[0], null);
                b.method(r.httpMethod(), body);
                try (okhttp3.Response result = client.newCall(b.build()).execute()) {
                    return new Response(result.code(), result.message(), result.headers().toMultimap(),
                        result.body() == null ? "" : result.body().string(), result.request().url().toString());
                }
            }
        });
    }
}
