package com.synaelle.ficelle.dev

import android.app.Activity
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.view.View
import android.view.ViewGroup
import android.webkit.JavascriptInterface

/**
 * Keeps the notes out of sight on a return after they may have locked, as
 * window.AppCover. Back in front, Android first shows its picture of the app
 * from when it left, then the web view its last frame, both the notes,
 * before the page can find the lock due. While the notes lock on their own
 * Android takes no picture (the recent apps show a blank card, from Android
 * 13), and a plain cover in the page's colour lies over the web view from
 * the moment the app is stopped until the page has drawn what it shows now
 * (lib/autolock.svelte.ts).
 */
class LockCover(private val activity: Activity) {
  @Volatile private var armed = false
  private var cover: View? = null
  private val handler = Handler(Looper.getMainLooper())
  private val remove = Runnable {
    cover?.let { (it.parent as? ViewGroup)?.removeView(it) }
    cover = null
  }

  /** Whether the notes are open and lock on their own after a while. */
  @JavascriptInterface
  fun arm(on: Boolean) {
    armed = on
    if (Build.VERSION.SDK_INT >= 33) activity.runOnUiThread { activity.setRecentsScreenshotEnabled(!on) }
  }

  /** The page has drawn what it shows now: the lock screen or the notes. */
  @JavascriptInterface
  fun lift() {
    handler.removeCallbacks(remove)
    handler.post(remove)
  }

  fun stop() {
    if (!armed || cover != null) return
    val color = (activity.window.decorView.background as? ColorDrawable)?.color ?: Color.BLACK
    val view = View(activity).apply {
      setBackgroundColor(color)
      // touches would reach the notes under it
      isClickable = true
    }
    activity.findViewById<ViewGroup>(android.R.id.content).addView(
      view,
      ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT),
    )
    cover = view
  }

  fun start() {
    // a page that never answers leaves the app blank for no longer than this
    if (cover != null) handler.postDelayed(remove, LIFT_AT_LATEST_MS)
  }

  private companion object {
    const val LIFT_AT_LATEST_MS = 1500L
  }
}
