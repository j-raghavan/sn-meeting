package dev.jraghavan.snmeeting

import android.app.Activity
import android.content.ContentValues
import android.os.Build
import android.provider.MediaStore
import dev.jraghavan.snmeeting.notes.PenInk

/**
 * F23-FR11: Settings → About → "Save diagnostics" on a tablet that isn't a Supernote, whose adb
 * may refuse shell and logcat (the Viwoods). Writes Download/meetings-diagnostics-<time>.txt:
 * the screen, the scale Meetings uses, the pen ink it found, and Meetings' own recent log (an
 * app may always read its own). The log never holds a key (INV2); nothing is sent anywhere.
 */
object Diagnostics {
    /** The report's text; JVM-tested. */
    fun report(fields: List<Pair<String, Any?>>, log: String): String = buildString {
        fields.forEach { (k, v) -> append(k).append(" = ").append(v).append('\n') }
        append("\n== Meetings log\n").append(log)
    }

    fun fileName(epochMs: Long): String = "meetings-diagnostics-$epochMs.txt"

    /** Collects and saves the report; returns the file name, or throws. Off the main thread. */
    fun save(a: Activity): String {
        val m = a.resources.displayMetrics
        val c = a.resources.configuration
        val fields = listOf(
            "meetings" to "${a.packageManager.getPackageInfo(a.packageName, 0).versionName} (core ${NativeBridge.nativeVersion()})",
            "device" to "${Build.MANUFACTURER} ${Build.MODEL} / Android ${Build.VERSION.RELEASE} (SDK ${Build.VERSION.SDK_INT}) / ${Build.DISPLAY}",
            "screen px" to "${m.widthPixels} x ${m.heightPixels}",
            "density / densityDpi" to "${m.density} / ${m.densityDpi}",
            "xdpi / ydpi" to "${m.xdpi} / ${m.ydpi}",
            "smallestWidthDp / screen dp" to "${c.smallestScreenWidthDp} / ${c.screenWidthDp} x ${c.screenHeightDp}",
            "DeviceScale factor / density" to "${DeviceScale.factor(a.resources)} / ${DeviceScale.density(a.resources)}",
            "pen ink" to PenInk.describe(a),
        )
        val name = fileName(System.currentTimeMillis())
        val values = ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, name)
            put(MediaStore.Downloads.MIME_TYPE, "text/plain")
        }
        val uri = a.contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values) ?: error("can't create $name")
        a.contentResolver.openOutputStream(uri, "wt")!!.use { it.write(report(fields, ownLog()).toByteArray()) }
        return name
    }

    /** This app's recent log lines (Android shows an app only its own). */
    private fun ownLog(): String = try {
        val p = ProcessBuilder("logcat", "-d", "-t", LOG_LINES.toString()).redirectErrorStream(true).start()
        p.inputStream.bufferedReader().use { it.readText() }
    } catch (t: Throwable) {
        "logcat failed: ${t.javaClass.simpleName}"
    }

    private const val LOG_LINES = 1000
}
