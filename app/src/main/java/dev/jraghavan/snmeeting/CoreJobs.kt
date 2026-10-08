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
class Jobs(
    private val worker: Executor,
    private val post: (Runnable) -> Unit,
    /** Where a swallowed failure is noted: its class only, never a message (it may hold a path). */
    private val log: (String) -> Unit = {},
) {
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
            log("a core call failed: ${t.javaClass.simpleName}")
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
        Jobs(worker, { main.post(it) }) { Log.w("Meetings", it) }
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

/**
 * Long jobs (transcribe, summarise, Test connection; M2/M3 ruling) on their own single thread,
 * so a minutes-long job never delays a tick or a rename on [CoreJobs]. It knows which keys are
 * running, so a screen rebuilt mid-job still shows its progress, and tells listeners when a job
 * ends (Home refreshes, F6-FR4).
 */
class JobLane(private val jobs: Jobs) {
    /** One per queued or running job; a cancelled one that hasn't started never runs (N9). */
    private class Ticket {
        var started = false
        var cancelled = false
    }

    private val tickets = HashMap<String, Ticket>()
    private val listeners = LinkedHashSet<(String) -> Unit>()

    fun isRunning(key: String) = synchronized(tickets) { key in tickets }

    /** Refuses a second job for the same key (one job per meeting): false then. Main thread. */
    fun run(key: String, work: () -> String, done: (String) -> Unit): Boolean {
        val t = Ticket()
        synchronized(tickets) {
            if (key in tickets) return false
            tickets[key] = t
        }
        jobs.run({
            val go = synchronized(tickets) { if (!t.cancelled) t.started = true; t.started }
            if (go) work() else CANCELLED
        }) { r ->
            if (t.cancelled) return@run // already off the lane, listeners told
            synchronized(tickets) { if (tickets[key] === t) tickets.remove(key) }
            done(r)
            listeners.toList().forEach { it(key) }
        }
        return true
    }

    /**
     * N9: a job still waiting behind another is taken off the lane: it never runs and its
     * [run] callback never fires; listeners hear it ended. False when there is no such job or it
     * has already started (cancel that one in the core). Main thread.
     */
    fun cancelQueued(key: String): Boolean {
        synchronized(tickets) {
            val t = tickets[key] ?: return false
            if (t.started) return false
            t.cancelled = true
            tickets.remove(key)
        }
        listeners.toList().forEach { it(key) }
        return true
    }

    /** Main thread; the returned function unsubscribes. */
    fun onFinished(l: (String) -> Unit): () -> Unit {
        listeners += l
        return { listeners -= l }
    }

    private companion object {
        const val CANCELLED = """{"error":"Cancelled"}"""
    }
}

object LongJobs {
    val lane by lazy {
        val worker = Executors.newSingleThreadExecutor { r -> Thread(r, "meeting-jobs").apply { isDaemon = true } }
        val main = Handler(Looper.getMainLooper())
        JobLane(Jobs(worker, { main.post(it) }) { Log.w("Meetings", it) })
    }
}

/**
 * S16: the template editor's Preview on its own thread. A preview renders a layout the user is
 * still writing, which can take a while; on [CoreJobs] it would delay a tick or the recorder's
 * next segment.
 */
object PreviewJobs {
    private val jobs by lazy {
        val worker = Executors.newSingleThreadExecutor { r -> Thread(r, "meeting-preview").apply { isDaemon = true } }
        val main = Handler(Looper.getMainLooper())
        Jobs(worker, { main.post(it) }) { Log.w("Meetings", it) }
    }

    fun run(work: () -> String, done: (String) -> Unit) = jobs.run(work, done)
}
