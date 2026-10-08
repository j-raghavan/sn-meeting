package dev.jraghavan.snmeeting.model

/**
 * S1: first-run permissions as decisions (JVM-tested). All-files access is checked on its own,
 * on every resume, and never opens the system screen by itself: Home shows a card with a Grant
 * access pill instead. The microphone is asked for only when the user taps ＋ New; once Android
 * has stopped showing the prompt (denied twice), Home says where to allow it.
 */
class Permissions {
    enum class OnResume { STORAGE_CARD, LISTS }
    enum class OnNew { GRANT_STORAGE, ASK_MIC, MIC_BLOCKED, START }

    private var micDenials = 0

    fun onResume(storage: Boolean): OnResume = if (storage) OnResume.LISTS else OnResume.STORAGE_CARD

    fun onNew(mic: Boolean, storage: Boolean): OnNew = when {
        !storage -> OnNew.GRANT_STORAGE
        mic -> OnNew.START
        micDenials >= 2 -> OnNew.MIC_BLOCKED
        else -> OnNew.ASK_MIC
    }

    fun micResult(granted: Boolean) {
        micDenials = if (granted) 0 else micDenials + 1
    }

    companion object {
        const val STORAGE_TEXT = "Meetings needs All-files access to read and save recordings in EXPORT/sn-meeting."
        const val MIC_BLOCKED_TEXT = "Allow the microphone for Meetings in Android Settings › Apps › Meetings › Permissions."
    }
}
