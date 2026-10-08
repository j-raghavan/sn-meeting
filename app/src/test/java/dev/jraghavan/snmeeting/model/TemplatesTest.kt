package dev.jraghavan.snmeeting.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class TemplatesTest {
    private fun row(id: String, origin: TemplateOrigin = TemplateOrigin.BUILTIN, problem: String? = null) =
        TemplateRow(id, id.replaceFirstChar { it.uppercase() }, "d", emptyList(), emptyList(), "i", "l", origin, origin != TemplateOrigin.YOURS, problem, null)
    private val rows = listOf(row("general"), row("standup", TemplateOrigin.CUSTOMISED, "Your copy has an error on line 3; using the built-in"),
        row("retro", TemplateOrigin.YOURS), row("broken", TemplateOrigin.YOURS, "This file has an error on line 9; it can't be used"))

    @Test fun theMeetingsTemplateElseTheDefault() {
        assertEquals("retro", TemplateChoice.of("retro", "general", rows).current!!.id)
        assertEquals("standup", TemplateChoice.of(null, "standup", rows).current!!.id)
    }

    @Test fun aBrokenYoursTemplateIsNotOffered() {
        assertEquals(listOf("general", "standup", "retro"), TemplateChoice.of(null, "general", rows).options.map { it.id })
    }

    @Test fun aGoneTemplateFallsBackToGeneralWithANote() {
        val c = TemplateChoice.of("deleted", "general", rows)
        assertEquals("general", c.current!!.id)
        assertEquals("Its template is gone, so General is used", c.note)
        assertNull(TemplateChoice.of(null, "deleted", rows).note) // Settings' default: the core already falls back
    }
}
