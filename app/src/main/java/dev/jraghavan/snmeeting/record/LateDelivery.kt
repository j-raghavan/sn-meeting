package dev.jraghavan.snmeeting.record

/**
 * S12: [r] on the recording thread through [post], or right here (the core's worker) when
 * [post] refuses because that thread has quit: the service was destroyed while a core call was
 * in flight, and its reply must still finish or discard the meeting.
 */
fun postOrRun(post: (Runnable) -> Boolean, r: Runnable) {
    if (!post(r)) r.run()
}

/**
 * S12: what nativeRecover is told about the service. A destroyed service stays "running" until
 * the core calls its recorder queued on the way out (close, finish) have run, so recovery never
 * moves the meeting on underneath them; [destroyed] clears it from the back of that queue.
 */
class RunningFlag {
    @Volatile var running = false
        private set

    fun created() { running = true }

    fun destroyed(queue: (() -> Unit) -> Unit) = queue { running = false }
}
