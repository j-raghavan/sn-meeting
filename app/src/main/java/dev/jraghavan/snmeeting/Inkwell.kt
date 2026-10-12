package dev.jraghavan.snmeeting

import android.app.Activity
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.text.Spannable
import android.text.SpannableString
import android.text.style.RelativeSizeSpan
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

/**
 * inkread's "Inkwell" design language (inkread HomeActivity / styles.xml), shared by every
 * Meetings screen (spec F9-FR3): script wordmark, serif body, mono letter-spaced eyebrows,
 * 1 dp black keylines, white pill and solid-black buttons, white paper, no animations.
 */
class Inkwell(private val activity: Activity) {

    /** F23: Android's density times the device factor ([DeviceScale]; 1 on the Manta). */
    private val density get() = DeviceScale.density(activity.resources)
    private val factor get() = DeviceScale.factor(activity.resources)
    fun dp(v: Int) = (v * density).toInt()
    val phi = 1.618f

    /** Pinyon Script (OFL, shared with inkread); bold = synthetic (single-weight font). */
    val script: Typeface by lazy { Typeface.createFromAsset(activity.assets, "fonts/pinyon_script.ttf") }
    val scriptBold: Typeface by lazy { Typeface.create(script, Typeface.BOLD) }
    val serif: Typeface = Typeface.SERIF
    val serifBold: Typeface = Typeface.create(Typeface.SERIF, Typeface.BOLD)
    val serifItalic: Typeface = Typeface.create(Typeface.SERIF, Typeface.ITALIC)
    val mono: Typeface = Typeface.MONOSPACE

    val ink = Color.BLACK
    val inkSoft = Color.parseColor("#3A3A3A")
    val textMuted = Color.parseColor("#757575")

    /** Side margin (≈ W/φ⁵) and width-driven scale against inkread's ~716-unit design column. */
    var side = 0
        private set
    var contentW = 0
        private set
    private var scale = 1f

    init {
        val w = activity.resources.displayMetrics.widthPixels
        side = (w / (phi * phi * phi * phi * phi)).toInt().coerceIn(dp(20), dp(52))
        contentW = (w - 2 * side).coerceAtLeast(dp(280))
        scale = (contentW.toFloat() / dp(716)).coerceIn(0.62f, 1.15f)
    }

    fun dim(designUnits: Number) = dp((designUnits.toFloat() * scale).toInt()).coerceAtLeast(1)
    /** Text in sp: [scale] is computed in device-scaled dp, so the factor comes back here and a
     *  width-driven screen reads the same on every device. */
    fun fs(designUnits: Number) = designUnits.toFloat() * scale * factor
    private val keyline get() = maxOf(1, dp(1))

    // ---- page scaffolding ----

    fun column(): LinearLayout = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.CENTER_HORIZONTAL
        setPadding(side, dim(40), side, dim(40))
        layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
    }

    fun page(column: LinearLayout): ScrollView = ScrollView(activity).apply {
        isFillViewport = true
        isVerticalScrollBarEnabled = false
        setBackgroundColor(Color.WHITE)
        addView(column)
    }

    fun spacer(h: Int): View = View(activity).apply {
        layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, h)
    }

    fun spacerWeighted(weight: Float): View = View(activity).apply {
        layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, weight)
    }

    fun fullWidth(): LinearLayout.LayoutParams =
        LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)

    fun wrap(): LinearLayout.LayoutParams =
        LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT)

    // ---- type ----

    /**
     * The "Meetings" wordmark: spaced serif capitals (owner decision 2026-10-09, option C). Its
     * styling lives here only, so a new typeface is a one-line change. The footer tagline stays
     * in Pinyon Script ([closingMark]).
     */
    fun wordmark(text: String, size: Float = 24f): TextView = TextView(activity).apply {
        this.text = text.uppercase(); setTextColor(ink); textSize = fs(size)
        typeface = serifBold; letterSpacing = 0.32f; gravity = Gravity.CENTER
        includeFontPadding = false
    }

    /** Home's lockup: the app icon left of the wordmark, on one centred line. */
    fun lockup(size: Float = 24f, icon: Int = dim(44)): View = LinearLayout(activity).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER
        addView(appIcon(), LinearLayout.LayoutParams(icon, icon))
        addView(wordmark("Meetings", size), wrap().apply { marginStart = dim(12) })
    }

    /** The app icon over the wordmark and its eyebrow, as on inkread's home screen. */
    fun brandBlock(eyebrowText: String, size: Float = 26f): View = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.CENTER_HORIZONTAL
        val logo = dim(56)
        addView(appIcon(), LinearLayout.LayoutParams(logo, logo).apply { bottomMargin = dim(8) })
        addView(wordmark("Meetings", size))
        addView(eyebrow(eyebrowText).apply { gravity = Gravity.CENTER; setPadding(0, dim(10), 0, 0) })
    }

    fun eyebrow(text: String, color: Int = ink): TextView = TextView(activity).apply {
        this.text = text.uppercase(); setTextColor(color); textSize = fs(12f)
        typeface = mono; letterSpacing = 0.12f
    }

    fun eyebrowItalic(text: String): TextView = TextView(activity).apply {
        this.text = text; setTextColor(inkSoft); textSize = fs(16f); gravity = Gravity.CENTER
        typeface = serifItalic
    }

    fun body(text: CharSequence, size: Float = 16f, bold: Boolean = false): TextView = TextView(activity).apply {
        this.text = text; setTextColor(ink); textSize = fs(size)
        typeface = if (bold) serifBold else serif
    }

    fun note(text: CharSequence, size: Float = 14f, color: Int = inkSoft): TextView = TextView(activity).apply {
        this.text = text; setTextColor(color); textSize = fs(size); typeface = serifItalic
    }

    /** Enlarge the capital letters (swash initials), as inkread's wordmark does. */
    fun swashCaps(text: String, factor: Float = 1.45f): CharSequence {
        val s = SpannableString(text)
        text.forEachIndexed { i, c ->
            if (c.isUpperCase()) s.setSpan(RelativeSizeSpan(factor), i, i + 1, Spannable.SPAN_EXCLUSIVE_EXCLUSIVE)
        }
        return s
    }

    // ---- shapes ----

    fun outline(radius: Int = dim(6), fill: Int = Color.WHITE): GradientDrawable = GradientDrawable().apply {
        setColor(fill); setStroke(keyline, ink); cornerRadius = radius.toFloat()
    }

    fun rule(width: Int = dim(40)): View = View(activity).apply {
        setBackgroundColor(ink)
        layoutParams = LinearLayout.LayoutParams(width, keyline)
    }

    fun hairline(): View = View(activity).apply {
        setBackgroundColor(ink)
        layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, keyline)
    }

    /** Outlined card (inkread's Daily strip): white, 1 dp black keyline, radius 6. */
    fun card(padH: Int = dim(24), padV: Int = dim(18)): LinearLayout = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(padH, padV, padH, padV)
        background = outline()
        layoutParams = fullWidth()
    }

    /**
     * Outlined card whose content scrolls inside a fixed height (the parent sets the height).
     * The scroll bar is a thin mid-grey thumb that never fades (a fading bar is never seen on
     * e-ink), shown only when the list overflows by more than a row's padding.
     */
    fun scrollCard(rows: LinearLayout): LinearLayout = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        background = outline()
        setPadding(dim(24), dim(6), dim(10), dim(6))
        val scroll = ScrollView(activity).apply {
            isFillViewport = false
            isVerticalScrollBarEnabled = true
            isScrollbarFadingEnabled = false
            scrollBarStyle = View.SCROLLBARS_OUTSIDE_OVERLAY
            scrollBarSize = maxOf(2, dp(2))
            verticalScrollbarThumbDrawable = GradientDrawable().apply { setColor(Color.parseColor("#8A8A8A")); cornerRadius = dp(1).toFloat() }
            verticalScrollbarTrackDrawable = null
            overScrollMode = View.OVER_SCROLL_NEVER
            rows.setPadding(0, 0, dim(14), 0) // keep text clear of the bar
            addView(rows, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        }
        addView(scroll, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))

        // An explicit cue, because a thin bar alone is easy to miss on e-ink: "SCROLL FOR MORE ↓"
        // while there is more below, "↑ BACK TO TOP" (tappable) once the end is reached, nothing
        // when the list fits. It redraws only when that state changes, never per scroll pixel.
        val cueRule = hairline().apply { visibility = View.GONE }
        val cue = eyebrow("", textMuted).apply {
            gravity = Gravity.CENTER; visibility = View.GONE
            setPadding(0, dim(10), dim(14), dim(6))
            setOnClickListener { scroll.scrollTo(0, 0) }
        }
        addView(cueRule)
        addView(cue, fullWidth())
        var state = -1 // 0 = fits, 1 = more below, 2 = at end
        fun refresh() {
            val overflow = (scroll.getChildAt(0)?.height ?: 0) - scroll.height
            val next = when {
                overflow <= 0 -> 0
                scroll.canScrollVertically(1) -> 1
                else -> 2
            }
            if (next == state) return
            state = next
            scroll.isVerticalScrollBarEnabled = overflow > dim(24) // a near-full thumb reads as a rule
            cue.visibility = if (next == 0) View.GONE else View.VISIBLE
            cueRule.visibility = cue.visibility
            cue.text = if (next == 1) "SCROLL FOR MORE  ↓" else "↑  BACK TO TOP"
            cue.isClickable = next == 2
        }
        scroll.addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ -> scroll.post { refresh() } }
        scroll.setOnScrollChangeListener { _, _, _, _, _ -> refresh() }
    }

    // ---- controls ----

    /** White pill with a black keyline (inkread's "Open a Document"); [filled] = solid black (its "Open →"). */
    fun pill(text: String, filled: Boolean = false, size: Float = 18f, onClick: () -> Unit): TextView =
        TextView(activity).apply {
            textSize = fs(size); typeface = serifBold; gravity = Gravity.CENTER
            setPadding(dim(32), dim(13), dim(32), dim(13))
            isClickable = true; setOnClickListener { onClick() }
            stylePill(this, text, filled)
        }

    fun stylePill(view: TextView, text: String, filled: Boolean) {
        view.text = text
        view.setTextColor(if (filled) Color.WHITE else ink)
        view.background = outline(dim(40), if (filled) ink else Color.WHITE)
    }

    /** Small mono status chip; [filled] inverts it (used for states that need attention). */
    fun chip(text: String, filled: Boolean = false): TextView = TextView(activity).apply {
        this.text = text.uppercase(); textSize = fs(10.5f); typeface = mono; letterSpacing = 0.12f
        setTextColor(if (filled) Color.WHITE else ink)
        setPadding(dim(10), dim(4), dim(10), dim(4))
        background = outline(dim(4), if (filled) ink else Color.WHITE)
    }

    /** Keyline checkbox: empty square, or solid black with a white tick. */
    fun checkbox(checked: Boolean): TextView = TextView(activity).apply {
        val s = dim(26)
        layoutParams = LinearLayout.LayoutParams(s, s)
        gravity = Gravity.CENTER; textSize = fs(15f); typeface = serifBold
        text = if (checked) "✓" else ""
        setTextColor(Color.WHITE)
        background = outline(dim(3), if (checked) ink else Color.WHITE)
    }

    /** Keyline radio: an outlined circle, with a solid black dot when selected. */
    fun radio(selected: Boolean): View = View(activity).apply {
        val s = dim(22)
        layoutParams = LinearLayout.LayoutParams(s, s)
        background = GradientDrawable().apply {
            shape = GradientDrawable.OVAL
            setColor(Color.WHITE); setStroke(maxOf(1, dp(2)), ink)
        }
        if (selected) foreground = android.graphics.drawable.InsetDrawable(
            GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(ink) }, dim(5))
    }

    /** One choice row: radio, serif label, optional italic note; muted and inert when [enabled] is false. */
    fun choiceRow(label: String, hint: String?, selected: Boolean, enabled: Boolean = true, onPick: () -> Unit): LinearLayout =
        LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, dim(10), 0, dim(10))
            addView(radio(selected).apply { alpha = if (enabled) 1f else 0.35f })
            addView(LinearLayout(activity).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(dim(14), 0, 0, 0)
                addView(body(label, 17f).apply { if (!enabled) setTextColor(textMuted) })
                if (hint != null) addView(note(hint, 13f, if (enabled) inkSoft else textMuted), wrap().apply { topMargin = dim(2) })
            }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            if (enabled) { isClickable = true; setOnClickListener { onPick() } }
        }

    /** Plain keyline text field (no underline, no ripple); [mono] for layouts and code-like text. */
    fun field(text: String, lines: Int = 1, mono: Boolean = false): android.widget.EditText =
        android.widget.EditText(activity).apply {
            setText(text); setTextColor(ink); textSize = fs(if (mono) 13f else 16f)
            typeface = if (mono) this@Inkwell.mono else serif
            background = outline(dim(4))
            setPadding(dim(14), dim(10), dim(14), dim(10))
            gravity = Gravity.TOP or Gravity.START
            if (lines > 1) { minLines = lines; isSingleLine = false } else isSingleLine = true
        }

    /**
     * The back control: just a house icon, no border (owner, 2026-10-09: "Just Home Icon,
     * Pressable"), in a pressable area of at least 56 dp, never under the 48 dp touch minimum
     * (model.BackControl); it finishes the screen.
     */
    fun backControl(to: String, onBack: () -> Unit = { activity.finish() }): View =
        android.widget.ImageView(activity).apply {
            val bc = dev.jraghavan.snmeeting.model.BackControl
            setImageResource(R.drawable.ic_home)
            setColorFilter(ink)
            scaleType = android.widget.ImageView.ScaleType.FIT_CENTER
            val size = maxOf(dp(bc.BUTTON_DP), bc.touchPx(density))
            val pad = (size - dp(bc.ICON_DP)) / 2
            setPadding(pad, pad, pad, pad)
            minimumWidth = size; minimumHeight = size
            layoutParams = LinearLayout.LayoutParams(size, size)
            contentDescription = bc.label(to)
            isClickable = true
            setOnClickListener { onBack() }
        }

    /** Screen header: the back control on the left, small wordmark centred (meeting, settings, templates). */
    fun header(back: String): View = LinearLayout(activity).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        addView(LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            addView(backControl(back))
        }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        addView(wordmark("Meetings", 18f))
        addView(View(activity), LinearLayout.LayoutParams(0, 1, 1f))
        layoutParams = fullWidth()
    }

    /** A section: mono eyebrow, then an outlined card the caller fills. Returns the card. */
    fun section(parent: LinearLayout, title: String): LinearLayout {
        parent.addView(spacer(dim(26)))
        parent.addView(eyebrow(title), fullWidth())
        parent.addView(spacer(dim(10)))
        return card(padV = dim(8)).also { parent.addView(it) }
    }

    /** The launcher icon, as inkread's home and footer show it. */
    fun appIcon(): ImageView = ImageView(activity).apply { setImageResource(R.mipmap.ic_launcher) }

    /** Muted script tagline over a rule · icon · rule line (inkread's closingMark). */
    fun closingMark(tagline: String = "Every Word, Kept"): View = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.CENTER_HORIZONTAL
        addView(TextView(activity).apply {
            text = swashCaps(tagline)
            setTextColor(textMuted); textSize = fs(12f); typeface = script
            gravity = Gravity.CENTER; includeFontPadding = false
        })
        addView(LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
            setPadding(0, dim(6), 0, 0)
            addView(rule())
            addView(appIcon(), LinearLayout.LayoutParams(dim(20), dim(20)).apply { marginStart = dim(12); marginEnd = dim(12) })
            addView(rule())
        })
    }

    fun versionName(): String =
        try { activity.packageManager.getPackageInfo(activity.packageName, 0).versionName ?: "" } catch (e: Exception) { "" }
}
