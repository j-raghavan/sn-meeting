package dev.jraghavan.snmeeting

import android.os.Handler
import android.os.Looper
import android.util.Log
import dev.jraghavan.snmeeting.record.Cancel
import org.json.JSONObject
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** `nativeLiveDrain`'s reply (F15-FR5). */
data class LiveReply(val state: String, val done: Int, val waiting: Int, val retryInMs: Long?, val reason: String?) {
    /** F15-FR7 on Home's recording line: "4 waiting to send" while the lane backs off,
     *  "transcription paused" once paused; null while it keeps up. */
    val note: String? get() = when (state) {
        "retry" -> "$waiting waiting to send"
        "paused" -> "transcription paused"
        else -> null
    }

    companion object {
        fun of(json: String): LiveReply = try {
            val o = JSONObject(json)
            if (o.has("error")) LiveReply("error", 0, 0, null, o.optString("error"))
            else LiveReply(o.optString("state", "error"), o.optInt("done"), o.optInt("waiting"),
                if (o.has("retryInMs")) o.getLong("retryInMs") else null, if (o.has("reason")) o.getString("reason") else null)
        } catch (e: Exception) {
            LiveReply("error", 0, 0, null, null)
        }
    }
}

/**
 * F15-FR5 / FR7: the near-live lane, a third lane beside CoreJobs and the job lane (M2/M3
 * ruling). A finalised chunk kicks it; one pass sends every pending chunk in order. After a
 * failure it waits the back-off the core names (5 s, 15 s, 60 s…) on its own schedule: kicks
 * meanwhile are ignored, so later chunks wait and the order holds (F15-AC3), and it never
 * watches the network (F15-FR7). A kick during a pass runs one more pass after it. Paused,
 * stopped or off ends it for that meeting. Any thread may call [kick] and [forget].
 */
class LiveLane(
    /** Runs a pass on the lane's one thread. */
    private val run: (() -> Unit) -> Unit,
    /** Runs [r] on the lane's thread after [ms]. */
    private val after: (ms: Long, r: () -> Unit) -> Cancel,
    private val drain: (String) -> String,
    /** Every pass's reply (the meeting page redraws its strip; Search re-indexes). */
    private val drained: (String, LiveReply) -> Unit = { _, _ -> },
) {
    private class Lane {
        var running = false
        var again = false
        var timer: Cancel? = null
    }

    private val lanes = HashMap<String, Lane>()

    /** A chunk of [id] was recorded. */
    fun kick(id: String) {
        synchronized(lanes) {
            val l = lanes.getOrPut(id) { Lane() }
            if (l.timer != null) return // backing off: the timer runs the next pass
            if (l.running) { l.again = true; return }
            l.running = true
        }
        run { pass(id) }
    }

    /** The recording ended: no further pass is scheduled (the Stop job takes over, F15-FR9). */
    fun forget(id: String) {
        synchronized(lanes) { lanes.remove(id)?.timer?.cancel() }
    }

    fun isBackingOff(id: String) = synchronized(lanes) { lanes[id]?.timer != null }

    private fun pass(id: String) {
        val r = LiveReply.of(try { drain(id) } catch (t: Throwable) { Jobs.errorJson(t) })
        try { drained(id, r) } catch (_: Throwable) {}
        var rerun = false
        synchronized(lanes) {
            val l = lanes[id] ?: return
            l.running = false
            when (r.state) {
                "retry" -> {
                    l.again = false
                    l.timer = after(r.retryInMs ?: RETRY_FALLBACK_MS) {
                        synchronized(lanes) { lanes[id]?.timer = null }
                        kick(id)
                    }
                }
                "idle", "blocked" -> { rerun = l.again; l.again = false }
                else -> lanes.remove(id) // paused, stopped, off, busy (the Stop job has it), error
            }
        }
        if (rerun) kick(id)
    }

    companion object {
        /** A retry with no wait named: the lane's longest back-off. */
        const val RETRY_FALLBACK_MS = 60_000L
    }
}

/** The app's near-live lane: one daemon thread, its own timers; replies observed on the main thread. */
object LiveJobs {
    private val exec = Executors.newSingleThreadScheduledExecutor { r -> Thread(r, "meeting-live").apply { isDaemon = true } }
    private val main by lazy { Handler(Looper.getMainLooper()) }
    private val listeners = LinkedHashSet<(String, LiveReply) -> Unit>()

    val lane = LiveLane(
        run = { r -> exec.execute { try { r() } catch (t: Throwable) { Log.w("Meetings", "live lane: ${t.javaClass.simpleName}") } } },
        after = { ms, r -> val f = exec.schedule({ r() }, ms, TimeUnit.MILLISECONDS); Cancel { f.cancel(false) } },
        drain = NativeBridge::nativeLiveDrain,
        drained = { id, r ->
            if (r.done > 0) SearchJobs.refresh() // F19-FR4: the new lines are searchable as they land
            main.post { listeners.toList().forEach { it(id, r) } }
        },
    )

    /** Main thread; the returned function unsubscribes. */
    fun observe(l: (String, LiveReply) -> Unit): () -> Unit {
        listeners += l
        return { listeners -= l }
    }
}
