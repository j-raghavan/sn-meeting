package dev.jraghavan.snmeeting

import org.junit.Assert.assertEquals
import org.junit.Test

class JobsTest {
    /** Runs work and delivery inline, recording the order of events. */
    private val log = mutableListOf<String>()
    private val jobs = Jobs(
        worker = { r -> log += "worker"; r.run() },
        post = { r -> log += "post"; r.run() },
    )

    @Test
    fun deliversTheResultThroughPost() {
        var got = ""
        jobs.run({ "{\"ok\":true}" }) { got = it; log += "done" }
        assertEquals("{\"ok\":true}", got)
        assertEquals(listOf("worker", "post", "done"), log)
    }

    @Test
    fun aThrowingJobBecomesAnErrorEnvelope() {
        var got = ""
        jobs.run({ throw UnsatisfiedLinkError("dlopen failed") }) { got = it }
        assertEquals("{\"error\":\"core unavailable: UnsatisfiedLinkError\"}", got)
    }
}
