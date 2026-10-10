// The in-place editor follows sn-canvas `android/app/src/main/java/com/sncanvas/canvas/
// TextEditRequest.kt` (MIT License, Copyright (c) 2026 JRaghavan; notice in LICENSES-3RDPARTY.md).
package dev.jraghavan.snmeeting.notes.canvas

import android.app.Activity
import android.content.Context
import android.graphics.Color
import android.text.InputType
import android.util.TypedValue
import android.view.Gravity
import android.view.KeyEvent
import android.view.WindowManager
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.FrameLayout

/**
 * DEVICE-UNVERIFIED (keyboard on the Manta). F21-FR9/FR10: a plain `EditText` laid over the
 * box (or table cell) being typed into, at the style's font size and in the serif the canvas
 * draws with; the window is set to `adjustNothing` so the canvas never resizes. Done or Back
 * commits; the keyboard opens only from the user's tap (never from a timer or a provider
 * result), with firmware ink off while it is open.
 */
class TextEditor(private val a: Activity, private val parent: FrameLayout) {
    /** What is being typed into: an item, and a table cell's index (null for a text box). */
    data class Target(val itemId: String?, val cell: Int?)

    private var box: EditText? = null
    private var target: Target? = null
    private var onDone: ((String) -> Unit)? = null
    private var softInputBefore: Int? = null

    val open: Boolean get() = box != null
    val editing: Target?
        get() = target

    private inner class Field(c: Context) : EditText(c) {
        override fun onKeyPreIme(keyCode: Int, event: KeyEvent): Boolean {
            if (keyCode == KeyEvent.KEYCODE_BACK && event.action == KeyEvent.ACTION_UP) {
                commit()
                return true
            }
            return super.onKeyPreIme(keyCode, event)
        }
    }

    /**
     * Opens the editor at the screen rectangle ([left], [top], [width]) with [initial] text at
     * [fontPx]; [done] gets the text when Done or Back commits it.
     */
    fun show(t: Target, left: Int, top: Int, width: Int, fontPx: Float, initial: String, done: (String) -> Unit) {
        close()
        target = t
        onDone = done
        softInputBefore = a.window.attributes.softInputMode
        a.window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING)
        val f = Field(a).apply {
            setText(initial)
            setSelection(initial.length)
            typeface = TextLayout.typeface
            setTextSize(TypedValue.COMPLEX_UNIT_PX, fontPx.coerceIn(12f, 120f))
            setTextColor(Color.BLACK)
            setBackgroundColor(Color.WHITE)
            gravity = Gravity.TOP or Gravity.START
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
            imeOptions = EditorInfo.IME_ACTION_DONE
            setHorizontallyScrolling(false)
            maxLines = 40
            setOnEditorActionListener { _, action, _ -> if (action == EditorInfo.IME_ACTION_DONE) { commit(); true } else false }
        }
        parent.addView(f, FrameLayout.LayoutParams(width.coerceAtLeast(MIN_WIDTH_PX), FrameLayout.LayoutParams.WRAP_CONTENT).apply {
            leftMargin = left.coerceAtLeast(0); topMargin = top.coerceAtLeast(0)
        })
        box = f
        f.requestFocus()
        (a.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager).showSoftInput(f, InputMethodManager.SHOW_IMPLICIT)
    }

    /** Commit what was typed (Done or Back). */
    fun commit() {
        val f = box ?: return
        val text = f.text.toString()
        val done = onDone
        close()
        done?.invoke(text)
    }

    /** Close without committing. */
    fun close() {
        val f = box ?: return
        (a.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager).hideSoftInputFromWindow(f.windowToken, 0)
        parent.removeView(f)
        box = null
        target = null
        onDone = null
        softInputBefore?.let { a.window.setSoftInputMode(it) }
        softInputBefore = null
    }

    private companion object {
        const val MIN_WIDTH_PX = 160
    }
}
