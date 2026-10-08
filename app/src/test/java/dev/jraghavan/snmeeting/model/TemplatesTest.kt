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

    @Test fun draftJsonAndVocabulary() {
        val d = TemplateDraft("retro", " Retro ", "d", "Priya, Arjun ,, Supernote", listOf("Wins"), "Summarise.", "{{ title }}")
        assertEquals(listOf("Priya", "Arjun", "Supernote"), d.vocabularyList())
        val o = org.json.JSONObject(d.json())
        assertEquals("Retro", o.getString("name"))
        assertEquals(3, o.getJSONArray("vocabulary").length())
        assertEquals("Wins", o.getJSONArray("sections").getString(0))
        assertEquals("{{ title }}", o.getString("layout"))
        assertEquals("Priya", TemplateDraft.of(row("x").copy(vocabulary = listOf("Priya"))).vocabulary)
    }

    @Test fun errorsReadAsLineN() {
        assertEquals("Line 12: The ~ operator isn't allowed", TemplateDraft.errorText(CoreResult.Err("The ~ operator isn't allowed", "layout", 12)))
        assertEquals("Template error on line 3: oops", TemplateDraft.errorText(CoreResult.Err("Template error on line 3: oops", "layout", 3)))
        assertEquals("The name must be 1–60 characters on one line", TemplateDraft.errorText(CoreResult.Err("The name must be 1–60 characters on one line", "name", null)))
    }

    @Test fun slugs() {
        assertEquals("weekly-retro", TemplateDraft.slug("Weekly Retro!", emptyList()))
        assertEquals("cafe-sync", TemplateDraft.slug("Café  sync", emptyList()))
        assertEquals("my-template", TemplateDraft.slug("会议", emptyList()))
        assertEquals("standup-2", TemplateDraft.slug("Standup", listOf("standup")))
        assertEquals("general-3", TemplateDraft.slug("General", listOf("general", "general-2")))
        assertEquals(56, TemplateDraft.slug("a".repeat(100), emptyList()).length)
    }
}
