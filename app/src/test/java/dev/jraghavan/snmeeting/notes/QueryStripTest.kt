package dev.jraghavan.snmeeting.notes

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class QueryStripTest { // F19-FR1 / FR9
    @Test fun theStripFitsTheProvidersSizeAndNeverGrows() {
        assertEquals(1568 to 200, QueryStrip.fit(3136, 400))
        assertEquals(1400 to 178, QueryStrip.fit(1400, 178))
        assertEquals(1568 to 130, QueryStrip.fit(1872, 156))
        assertEquals(0 to 0, QueryStrip.fit(0, 10))
    }

    @Test fun theReplyIsTheTextOrTheKeyboard() {
        assertEquals("budget", QueryStrip.text("""{"text":" budget "}"""))
        assertNull(QueryStrip.text("""{"text":"  "}"""))
        assertNull(QueryStrip.text("""{"error":"offline","offline":true}"""))
        assertNull(QueryStrip.text("not json"))
    }
}
