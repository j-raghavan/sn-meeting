package dev.jraghavan.snmeeting

import android.os.Handler
import android.os.Looper
import java.util.concurrent.Executor
import java.util.concurrent.Executors

/**
 * Runs blocking core calls off the UI thread (ADR D7): one background thread, so jobs run in
 * the order they were queued (nativeInit before the first Test connection), and each result is
 * delivered on [post]'s thread.
 */
class Jobs(private val worker: Executor, private val post: (Runnable) -> Unit) {
    fun run(work: () -> String, done: (String) -> Unit) {
        worker.execute {
            // A missing or broken libmeeting.so throws an Error from NativeBridge's initialiser;
            // turn it into the same envelope the core uses so callers handle one shape.
            val result = try {
                work()
            } catch (t: Throwable) {
                errorJson(t)
            }
            post(Runnable { done(result) })
        }
    }

    companion object {
        /** `{"error": "<exception class>"}`; the class name needs no JSON escaping. */
        fun errorJson(t: Throwable): String = "{\"error\":\"core unavailable: ${t.javaClass.simpleName}\"}"
    }
}

/** The app's job queue: a single daemon thread, results on the main looper. */
object CoreJobs {
    private val jobs by lazy {
        val worker = Executors.newSingleThreadExecutor { r -> Thread(r, "meeting-core").apply { isDaemon = true } }
        val main = Handler(Looper.getMainLooper())
        Jobs(worker) { main.post(it) }
    }

    fun run(work: () -> String, done: (String) -> Unit) = jobs.run(work, done)
}
