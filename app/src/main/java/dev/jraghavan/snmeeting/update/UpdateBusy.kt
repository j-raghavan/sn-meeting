package dev.jraghavan.snmeeting.update

import dev.jraghavan.snmeeting.LongJobs
import dev.jraghavan.snmeeting.record.RecordingService

/**
 * F14-FR8, a Meetings rule inkread doesn't need: installing an update kills the process, which
 * would end a recording or a transcription mid-way. So the updater never checks, prompts,
 * downloads or installs while [RecordingService] is alive or a transcribe/summarise job is on
 * the job lane, and the install receiver asks again just before Android's confirmation.
 */
object UpdateBusy {
    fun now(): Boolean = RecordingService.running || LongJobs.lane.hasMeetingJob()

    /** The line shown when an install is refused for that reason. */
    const val MESSAGE = "Finish the recording or transcription first, then install the update."
}
