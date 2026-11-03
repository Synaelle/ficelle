package com.synaelle.ficelle.dev

import android.app.Activity
import android.graphics.Bitmap
import android.graphics.Canvas
import android.util.Base64
import android.webkit.JavascriptInterface
import android.webkit.WebView
import java.io.ByteArrayOutputStream
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * The page as it shows now, as window.AppShot: for an error report, sent
 * only when the person ticks it, and for the dev reports.
 */
class Screenshot(private val activity: Activity, private val webView: WebView) {
  /** A PNG in base64, empty when it failed. */
  @JavascriptInterface
  fun take(): String {
    var png = ""
    val done = CountDownLatch(1)
    activity.runOnUiThread {
      try {
        if (webView.width > 0 && webView.height > 0) {
          val bitmap = Bitmap.createBitmap(webView.width, webView.height, Bitmap.Config.ARGB_8888)
          webView.draw(Canvas(bitmap))
          val out = ByteArrayOutputStream()
          bitmap.compress(Bitmap.CompressFormat.PNG, 100, out)
          bitmap.recycle()
          png = Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP)
        }
      } finally {
        done.countDown()
      }
    }
    done.await(3, TimeUnit.SECONDS)
    return png
  }
}
