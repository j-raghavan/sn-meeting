package dev.jraghavan.snmeeting.model

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class KeyImportTest {
    private fun o(s: String) = JSONObject(s)

    @Test fun files_are_read_with_their_folder() {
        val f = KeyImport.files(o("""{"files":[
            {"path":"/s/Download/copilot-key-openai.txt","name":"copilot-key-openai.txt","folder":"Download"},
            {"path":"/s/copilot-key-gemini.txt","name":"copilot-key-gemini.txt","folder":""},
            {"path":"/s/Documents/copilot-key-x.txt"},
            {"name":"no path"}]}"""))
        assertEquals(3, f.size)
        assertEquals("copilot-key-openai.txt · Download", f[0].label)
        assertEquals("copilot-key-gemini.txt", f[1].label)
        assertEquals(KeyImport.KeyFile("/s/Documents/copilot-key-x.txt", "copilot-key-x.txt", ""), f[2])
        assertEquals(emptyList<KeyImport.KeyFile>(), KeyImport.files(o("{}")))
    }

    @Test fun replies_are_imported_exists_or_unknown() {
        assertEquals(KeyImport.Reply.Imported("openai", false), KeyImport.reply(o("""{"imported":"openai","replaced":false}""")))
        assertEquals(KeyImport.Reply.Imported("gemini", true), KeyImport.reply(o("""{"imported":"gemini","replaced":true}""")))
        assertEquals(KeyImport.Reply.Imported("anthropic", false), KeyImport.reply(o("""{"imported":"anthropic"}""")))
        assertEquals(KeyImport.Reply.Exists("openai"), KeyImport.reply(o("""{"exists":"openai"}""")))
        assertNull(KeyImport.reply(o("""{"ok":true}""")))
    }

    @Test fun messages_name_the_provider_and_the_folder() {
        assertEquals("Meetings already has a key for OpenAI. Replace it with this file?", KeyImport.replaceQuestion("openai"))
        assertEquals("Google Gemini key replaced.", KeyImport.done(KeyImport.Reply.Imported("gemini", true)))
        val file = KeyImport.KeyFile("/s/Download/copilot-key-openai.txt", "copilot-key-openai.txt", "Download")
        assertEquals(
            "OpenAI key imported. Delete copilot-key-openai.txt from Download now that Meetings has its own copy?",
            KeyImport.deleteQuestion(KeyImport.Reply.Imported("openai", false), file),
        )
        assertEquals(
            "Anthropic key imported. Delete copilot-key-anthropic.txt from this tablet's storage now that Meetings has its own copy?",
            KeyImport.deleteQuestion(KeyImport.Reply.Imported("anthropic", false), file.copy(name = "copilot-key-anthropic.txt", folder = "")),
        )
    }
}
