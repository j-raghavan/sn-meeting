package dev.jraghavan.snmeeting.notes

import android.graphics.Canvas
import android.graphics.PixelFormat
import android.graphics.PorterDuff
import android.util.Log
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import dev.jraghavan.snmeeting.notes.canvas.FirmwarePen

/**
 * DEVICE-UNVERIFIED in Meetings (the same calls were tried on the Musnap Aura C in a test app,
 * 2026-10-10: fast, clean strokes). F23-FR4: the iReader e-ink platform's system hand-write
 * ("AHW"), all by reflection: an `android.eink.view.HWSurfaceView`, transparent and on top of
 * the canvas, whose `FBHandSurface` draws the stylus straight onto the panel. The surface takes
 * no touches, so the canvas below still gets every pen event and keeps the strokes, exactly as
 * with [SupernoteInk]; the canvas redraws and this ink is wiped only when [InkBake] says so,
 * never after each stroke (a refresh then would swallow the next letters). Never throws.
 */
class IreaderInk private constructor(private val canvas: View, private val surfaceClass: Class<*>) : PenInk {
    private var surface: SurfaceView? = null
    private var fb: Any? = null
    private var active = false
    private var writable = false

    override var onReady: (() -> Unit)? = null

    override var pen: FirmwarePen = FirmwarePen.NEEDLE_BLACK
        set(v) {
            if (v == field) return
            field = v
            if (active) sendPen()
        }

    override fun setup(reason: String): Boolean {
        val s = surface ?: attach() ?: return false
        if (fb == null) return false // its surface isn't there yet: onReady claims again
        val ok = configure(s)
        Log.i(TAG, "IreaderInk: setup ($reason) ok=$ok")
        return ok
    }

    /** The surface over the canvas, in the canvas's own frame; its FBHandSurface comes once the
     *  surface exists, and then [onReady] lets the canvas claim the pen. */
    private fun attach(): SurfaceView? {
        // Only over a canvas in its own frame (the Notes page); the Minutes tab's ink area sits
        // in a scrolling column, where it draws its own stroke as before.
        val parent = canvas.parent as? FrameLayout ?: return null
        return try {
            val s = surfaceClass.getConstructor(android.content.Context::class.java).newInstance(canvas.context) as SurfaceView
            s.setZOrderOnTop(true)
            s.holder.setFormat(PixelFormat.TRANSPARENT)
            s.isClickable = false
            s.isFocusable = false
            s.holder.addCallback(object : SurfaceHolder.Callback {
                override fun surfaceCreated(h: SurfaceHolder) {
                    // The FBHandSurface binds after this callback; ask on the next frame.
                    s.postDelayed({ ready(s) }, READY_DELAY_MS)
                }
                override fun surfaceChanged(h: SurfaceHolder, f: Int, w: Int, ht: Int) {}
                override fun surfaceDestroyed(h: SurfaceHolder) { fb = null; active = false }
            })
            parent.addView(s, parent.indexOfChild(canvas) + 1,
                FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
            surface = s
            s
        } catch (t: Throwable) {
            Log.w(TAG, "IreaderInk: surface failed: ${t.javaClass.simpleName}: ${t.message}")
            null
        }
    }

    private fun ready(s: SurfaceView) {
        if (surface !== s) return
        fb = try { s.javaClass.getMethod("getFBSurface").invoke(s) } catch (t: Throwable) { null }
        if (fb == null) { Log.w(TAG, "IreaderInk: no FBHandSurface"); return }
        clearAll()
        onReady?.invoke()
    }

    /** Bounds to the canvas on screen, the stylus only (fingers and palms never ink), the pen,
     *  and the hand-write on when the tool writes. */
    private fun configure(s: SurfaceView): Boolean {
        val f = fb ?: return false
        val loc = IntArray(2).also { s.getLocationOnScreen(it) }
        val ok = call(f, "setBounds", arrayOf(Int::class.java, Int::class.java, Int::class.java, Int::class.java),
            loc[0], loc[1], loc[0] + s.width, loc[1] + s.height) &&
            call(f, "setAhwToolTypes", arrayOf(IntArray::class.java), intArrayOf(AHW_TOOLTYPE_STYLUS))
        active = ok
        if (ok) { sendPen(); writable = true; auto(true) }
        return ok
    }

    private fun sendPen() {
        val f = fb ?: return
        val p = pen
        // Supernote's size 1000 (the needle, M) draws about 3 px here; colour is black or grey.
        val width = AHW_WIDTH_AT_M * p.size / FirmwarePen.CANVAS_SIZE
        val color = if (p.color == FirmwarePen.BLACK) PEN_BLACK else PEN_GRAY
        call(f, "setAhwPen", arrayOf(Int::class.java, Float::class.java, Int::class.java), PEN_TYPE_SIMPLE, width, color)
    }

    private fun auto(on: Boolean) { fb?.let { call(it, "setAutoHandWrite", arrayOf(Boolean::class.java), on) } }

    override fun clearAll() {
        val s = surface ?: return
        try {
            val c = s.javaClass.getMethod("lockActiveCanvas").invoke(s) as? Canvas ?: return
            c.drawColor(0, PorterDuff.Mode.CLEAR)
            s.javaClass.getMethod("unlockActiveCanvas").invoke(s)
        } catch (t: Throwable) {
            Log.w(TAG, "IreaderInk: clear failed: ${t.javaClass.simpleName}")
        }
    }

    override fun setWritable(enable: Boolean) {
        if (!active) return
        writable = enable
        auto(enable)
    }

    /** The surface covers only the canvas, so the bar, the top bar and the strip are already
     *  outside it; the controls over the canvas (the zoom pill, the minimap) are left to the
     *  canvas, which ignores pen strokes that begin on them. */
    override fun setDisabledAreas(rects: List<PxRect>, where: String) {}

    override fun teardown() {
        if (active) { auto(false); clearAll() }
        active = false; writable = false; fb = null
        surface?.let { s -> (s.parent as? ViewGroup)?.removeView(s) }
        surface = null
    }

    /** One full GC16 refresh of the window, as the platform's own ViewAPI.invalidate does:
     *  the root's post mode set, invalidate, the mode put back. */
    override fun fullRefresh() {
        val root = canvas.rootView
        try {
            val get = View::class.java.getMethod("getPostMode")
            val set = View::class.java.getMethod("setPostMode", Int::class.java)
            val old = get.invoke(root) as Int
            set.invoke(root, UI_EINK_GC16_FULL_MODE)
            root.invalidate()
            root.post { runCatching { set.invoke(root, old) } }
        } catch (t: Throwable) {
            root.invalidate()
        }
    }

    override fun systemGestures(enabled: Boolean) {}

    private fun call(target: Any, name: String, types: Array<Class<*>>, vararg args: Any): Boolean = try {
        target.javaClass.getMethod(name, *types).invoke(target, *args)
        true
    } catch (t: Throwable) {
        Log.w(TAG, "IreaderInk: $name failed: ${(t.cause ?: t).javaClass.simpleName}")
        false
    }

    companion object {
        private const val TAG = "MeetingsInk"
        private const val SURFACE_CLASS = "android.eink.view.HWSurfaceView"
        private const val READY_DELAY_MS = 300L
        // From the device's framework (android.eink.view.FBHandSurface / HandPenConfig, android.eink.api.EinkMode).
        private const val AHW_TOOLTYPE_STYLUS = 2
        private const val PEN_TYPE_SIMPLE = 0
        private const val PEN_BLACK = -16777216
        private const val PEN_GRAY = -11513776
        private const val UI_EINK_GC16_FULL_MODE = 98
        private const val AHW_WIDTH_AT_M = 3f

        /** The platform's hand-write, when this device has it (and no Supernote daemon). */
        fun create(canvas: View): IreaderInk? = try {
            IreaderInk(canvas, Class.forName(SURFACE_CLASS))
        } catch (t: Throwable) {
            null
        }
    }
}
