package dev.jraghavan.snmeeting

import org.junit.Assert.assertEquals
import org.junit.Test

class DiagnosticsTest { // F23-FR11
    @Test fun theReportListsEachFieldThenTheLog() {
        assertEquals(
            "screen px = 1440 x 1920\npen ink = none\n\n== Meetings log\nline 1\n",
            Diagnostics.report(listOf("screen px" to "1440 x 1920", "pen ink" to "none"), "line 1\n"),
        )
        assertEquals("meetings-diagnostics-42.txt", Diagnostics.fileName(42))
    }
}
