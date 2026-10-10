package dev.jraghavan.snmeeting.notes.canvas

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class StyleRulesTest { // F21-FR6: options that don't apply to the selection are dimmed
    @Test fun optionsApplyWhereTheSpecSays() {
        assertTrue(StyleRules.applies("stroke", "color", "red") && StyleRules.applies("stroke", "size", "l"))
        assertFalse(StyleRules.applies("stroke", "fill", "solid") || StyleRules.applies("stroke", "dash", "dashed"))
        assertTrue(StyleRules.applies("note", "fill", "semi") && !StyleRules.applies("text", "fill", "semi"))
        assertTrue(StyleRules.applies("text", "size", "l") && !StyleRules.applies("text", "opacity", "0.5"))
        assertTrue(StyleRules.applies("image", "dash", "none") && !StyleRules.applies("rectangle", "dash", "none"))
        assertFalse(StyleRules.applies("image", "size", "l") || StyleRules.applies("x", "glow", "1"))
    }

    @Test fun aSelectionDimsWhatNoneOfItTakes() {
        assertTrue(StyleRules.enabled(emptySet(), "fill", "solid"))
        assertFalse("no image to leave unframed", StyleRules.enabled(emptySet(), "dash", "none"))
        assertTrue(StyleRules.enabled(setOf("stroke", "rectangle"), "fill", "solid"))
        assertFalse(StyleRules.enabled(setOf("stroke", "text"), "fill", "solid"))
    }
}
