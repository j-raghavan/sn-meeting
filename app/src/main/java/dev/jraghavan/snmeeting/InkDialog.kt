package dev.jraghavan.snmeeting

import android.app.Activity
import android.app.AlertDialog
import android.graphics.Typeface
import android.text.InputType
import android.widget.FrameLayout
import android.widget.TextView

/**
 * Platform AlertDialogs in inkread's InkDialog theme (F9-FR4): black accents, serif title, mono
 * bold letter-spaced buttons, a rounded keyline card, no animation. Rename, delete, the
 * one-time provider notice and errors. Each is a no-op once its activity is going away.
 */
object InkDialog {
    private fun builder(a: Activity) = AlertDialog.Builder(a, R.style.InkDialog)

    private fun gone(a: Activity) = a.isFinishing || a.isDestroyed

    /** Serif body text, as on every screen (the theme styles only the title and buttons). */
    private fun AlertDialog.Builder.showSerif(): AlertDialog = show().also { d ->
        d.findViewById<TextView>(android.R.id.message)?.apply { typeface = Typeface.SERIF; setLineSpacing(0f, 1.25f) }
    }

    /** [onClose] runs however it closes (F21-FR3: the notes canvas re-claims the pen). */
    fun confirm(a: Activity, title: String, message: String, yes: String, onClose: () -> Unit = {}, onYes: () -> Unit) {
        if (gone(a)) return
        builder(a).setTitle(title).setMessage(message)
            .setPositiveButton(yes.uppercase()) { _, _ -> onYes() }
            .setNegativeButton("CANCEL", null)
            .setOnDismissListener { onClose() }
            .showSerif()
    }

    /** A message that needs an answer before going on; Cancel does nothing. */
    fun notice(a: Activity, title: String, message: String, ok: String, onOk: () -> Unit) {
        if (gone(a)) return
        builder(a).setTitle(title).setMessage(message).setCancelable(false)
            .setPositiveButton(ok.uppercase()) { _, _ -> onOk() }
            .setNegativeButton("CANCEL", null)
            .showSerif()
    }

    /** Three answers (the update prompt: Install / Skip this version / Later, F14-FR8). */
    fun choose(a: Activity, title: String, message: String, yes: String, no: String, later: String, onYes: () -> Unit, onNo: () -> Unit, onLater: () -> Unit) {
        if (gone(a)) return
        builder(a).setTitle(title).setMessage(message)
            .setPositiveButton(yes.uppercase()) { _, _ -> onYes() }
            .setNegativeButton(no.uppercase()) { _, _ -> onNo() }
            .setNeutralButton(later.uppercase()) { _, _ -> onLater() }
            .setOnCancelListener { onLater() }
            .showSerif()
    }

    /** One of [items], or nothing on Cancel (F24: which key file to import). */
    fun pick(a: Activity, title: String, items: List<String>, onPick: (Int) -> Unit) {
        if (gone(a)) return
        builder(a).setTitle(title)
            .setItems(items.toTypedArray()) { _, i -> onPick(i) }
            .setNegativeButton("CANCEL", null)
            .show()
    }

    /** A message with only OK (an event's details from Search, until its sheet exists). */
    fun info(a: Activity, title: String, message: String) = error(a, title, message)

    fun error(a: Activity, title: String, message: String) {
        if (gone(a)) return
        builder(a).setTitle(title).setMessage(message).setPositiveButton("OK", null).showSerif()
    }

    /** One-line text entry (rename). An empty result is allowed: it clears the title. */
    fun text(a: Activity, ui: Inkwell, title: String, initial: String, onSave: (String) -> Unit) {
        if (gone(a)) return
        val field = ui.field(initial).apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
            setSelection(text.length)
        }
        val box = FrameLayout(a).apply { setPadding(ui.dim(24), ui.dim(8), ui.dim(24), 0); addView(field) }
        builder(a).setTitle(title).setView(box)
            .setPositiveButton("SAVE") { _, _ -> onSave(field.text.toString().trim()) }
            .setNegativeButton("CANCEL", null)
            .showSerif()
    }
}
