package dev.jraghavan.snmeeting

import android.os.Handler
import android.os.Looper
import android.util.Log
import java.util.concurrent.Executor
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

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

    /** Fire and forget, still in queue order (the recorder's close/add/finish). Never throws. */
    fun exec(work: () -> Unit) = worker.execute {
        try {
            work()
        } catch (t: Throwable) {
            // A broken core can't be reported from here; the next call that replies will say so.
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
    private val initQueued = AtomicBoolean(false)

    fun run(work: () -> String, done: (String) -> Unit) = jobs.run(work, done)
    fun exec(work: () -> Unit) = jobs.exec(work)

    /**
     * Queues nativeInit once per process, ahead of anything queued after it. Every entry point
     * (an activity the system restores, the service) calls this first; nativeInit is idempotent.
     */
    fun init(appDataDir: String) {
        if (initQueued.compareAndSet(false, true)) {
            exec { Log.i("Meetings", "nativeInit: " + NativeBridge.nativeInit(appDataDir)) }
        }
    }
}
