// Adapted from inkread `app/src/main/java/dev/jraghavan/inkread/eink/SupernoteInk.kt` and
// `SupernoteEinkAdapter.kt` (AGPL-3.0-only, same owner; credited in LICENSES-3RDPARTY.md).
package dev.jraghavan.snmeeting.notes

import android.content.Context
import android.os.IBinder
import android.os.Parcel
import android.util.Log

/**
 * DEVICE-UNVERIFIED (needs the Supernote's pen daemon). Firmware pen ink (F17-FR4): the
 * `service_myservice` binder paints the stylus straight onto the e-ink overlay at the latency
 * of Supernote's own notes, so Meetings never draws the live pen stroke; it claims the pen,
 * clears the overlay, and commits each stroke when it ends. Reflection and binder only; never
 * throws. The contract (service name, token, transaction codes, parcel layout) is the one
 * inkread reproduces from plateaukao's AGPL-3 reimplementation of HandWriteClient.
 */
class SupernoteInk(private val context: Context) {
    private var cached: IBinder? = null
    private var active = false
    /** Whether the pen inks at all now (the Pen tool); the disabled areas apply only then. */
    private var writable = false
    /** F17-FR4: where the pen must never ink (screen px): every control, not the canvas. */
    private var areas: List<PxRect> = emptyList()

    private fun binder(): IBinder? {
        cached?.let { if (it.isBinderAlive) return it }
        cached = try {
            val get = Class.forName("android.os.ServiceManager").getMethod("getService", String::class.java)
            SERVICE_NAMES.firstNotNullOfOrNull { get.invoke(null, it) as? IBinder }
        } catch (t: Throwable) {
            Log.w(TAG, "ServiceManager.getService failed: ${t.javaClass.simpleName}")
            null
        }
        return cached
    }

    private fun send(code: Int, write: (Parcel) -> Unit) {
        val b = binder() ?: return
        val data = Parcel.obtain()
        val reply = Parcel.obtain()
        try {
            data.writeInterfaceToken(IFACE_TOKEN)
            data.writeString(APP_NAME)
            write(data)
            b.transact(code, data, reply, 0)
        } catch (t: Throwable) {
            Log.w(TAG, "transact($code) failed: ${t.javaClass.simpleName}")
        } finally {
            data.recycle(); reply.recycle()
        }
    }

    /** One EinkManager call by reflection; whether it went through. */
    private fun eink(method: String, enable: Boolean): Boolean {
        return try {
            val e = context.getSystemService("eink") ?: return false
            e.javaClass.getMethod(method, Boolean::class.javaPrimitiveType).invoke(e, enable)
            true
        } catch (t: Throwable) {
            Log.w(TAG, "$method($enable) failed: ${t.javaClass.simpleName}: ${t.message}")
            false
        }
    }

    /**
     * Claim the pen for this window; safe to repeat on every resume and focus gain ([reason] is
     * logged). inkread `SupernoteInk.setup`, call for call (app info, enableFullUiAuto(true),
     * disabled areas, the needle pen), except that the disabled areas are the last ones
     * [setDisabledAreas] was given rather than none.
     */
    fun setup(reason: String = ""): Boolean {
        val b = binder()
        if (b == null) {
            Log.i(TAG, "SupernoteInk: setup failed ($reason): binder=null")
            return false
        }
        send(TX_WRITE_APP_INFO) { it.writeInt(0); it.writeInt(0) }
        val fullAuto = eink("enableFullUiAuto", true)
        sendAreas()
        send(TX_PEN) { it.writeInt(PEN_NEEDLE); it.writeInt(SIZE_EMR); it.writeInt(COLOR_BLACK) }
        active = true
        writable = true
        Log.i(TAG, "SupernoteInk: setup ok ($reason): binder=${b.javaClass.simpleName} alive=${b.isBinderAlive}, fullAuto=$fullAuto")
        return true
    }

    /** Wipe the overlay (the canvas then redraws its own rendering of every stroke). */
    fun clearAll() {
        if (active) send(TX_DRAW_BUFFER) { it.writeInt(255); it.writeInt(0) }
    }

    /** Firmware ink on (except over [areas]) or off over the whole window (off for Eraser,
     *  Lasso and Listen). */
    fun setWritable(enable: Boolean) {
        if (!active) return
        Log.i(TAG, "SupernoteInk: writable=$enable")
        writable = enable
        val edge = if (enable) WRITABLE_ON_EDGE else WRITABLE_OFF_EDGE
        send(TX_DISABLE_AREA) { it.writeInt(1); it.writeInt(0); it.writeInt(0); it.writeInt(edge); it.writeInt(edge); it.writeInt(0) }
        if (enable) sendAreas()
    }

    /**
     * F17-FR4 (DEVICE-UNVERIFIED): the screen-pixel rectangles where the pen must not ink, the
     * tool bar, the top bar, the strip and the controls over the canvas ([InkAreas]). Sent now
     * while the pen inks, and again on every [setup] and [setWritable] (true). [where] is
     * logged with them, e.g. the window offset, so the space can be checked on the device.
     */
    fun setDisabledAreas(rects: List<PxRect>, where: String = "") {
        if (rects == areas) return
        areas = rects
        Log.i(TAG, "SupernoteInk: disabled areas ${InkAreas.describe(rects)} $where")
        if (active && writable) sendAreas()
    }

    /** TX_DISABLE_AREA with each rect as 5 ints `[left, top, width, height, 0]`. */
    private fun sendAreas() {
        val rs = areas
        send(TX_DISABLE_AREA) { p ->
            p.writeInt(rs.size)
            rs.forEach { r -> r.ltwh().forEach(p::writeInt); p.writeInt(0) }
        }
    }

    /** Release the pen when the page goes, ink off (inkread #157: else it inks other apps). */
    fun teardown() {
        if (!active) return
        clearAll()
        setWritable(false)
        eink("enableFullUiAuto", false)
        active = false
        writable = false
    }

    /** A full-panel refresh, only when the user asks (the bar's refresh option, F17-FR11). */
    fun fullRefresh() {
        try {
            val e = context.getSystemService("eink") ?: return
            Class.forName("android.os.EinkManager").getDeclaredMethod("sendOneFullFrame").invoke(e)
        } catch (t: Throwable) {
            Log.w(TAG, "sendOneFullFrame failed: ${t.javaClass.simpleName}")
        }
    }

    /** Hand full touch streams to the canvas: the system gesture service otherwise eats finger
     *  ups (inkread `SupernoteEinkAdapter.setSystemGesturesEnabled`). */
    fun systemGestures(enabled: Boolean) {
        eink("setGlobalGuestureEnabled", enabled)
        eink("setStylusGuestureEnabled", enabled)
    }

    private companion object {
        const val TAG = "MeetingsInk"
        val SERVICE_NAMES = arrayOf("service_myservice", "service.myservice")
        const val IFACE_TOKEN = "android.demo.IMyService"
        const val APP_NAME = "meetings"
        const val TX_WRITE_APP_INFO = 0
        const val TX_DISABLE_AREA = 1
        const val TX_PEN = 2
        const val TX_DRAW_BUFFER = 6
        const val PEN_NEEDLE = 10
        const val COLOR_BLACK = 0
        const val SIZE_EMR = 1000
        const val WRITABLE_ON_EDGE = 18888
        const val WRITABLE_OFF_EDGE = 19999
    }
}
