package dev.jraghavan.snmeeting.record

/**
 * The ports the recording logic depends on. Everything behind them is a thin Android adapter
 * (DEVICE-UNVERIFIED); everything in front is plain Kotlin with JVM tests.
 */
fun interface Cancel { fun cancel() }

/** Runs [r] on the recording thread after [ms]; the returned handle cancels it. */
fun interface TimerPort { fun after(ms: Long, r: () -> Unit): Cancel }
