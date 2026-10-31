package com.synaelle.ficelle.dev

import android.app.Activity
import android.graphics.Bitmap
import android.graphics.Canvas
import android.net.Uri
import android.provider.DocumentsContract
import android.util.Base64
import android.webkit.JavascriptInterface
import android.webkit.WebView
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * Issue reports of dev and preview builds, as window.AppReports. The app's
 * own storage is out of reach of a computer, so reports go to a folder the
 * person picks (a Syncthing folder, say), written through the system's
 * document access. Temporary, until the real feedback feature replaces it.
 * Release builds never add it (BuildConfig.DEV_REPORTS).
 */
class DevReports(
  private val activity: Activity,
  private val webView: WebView,
  private val pick: () -> Unit,
) {
  /** Report folders made this run, by name, for their images. */
  private val made = mutableMapOf<String, Uri>()

  private fun prefs() = activity.getSharedPreferences("devReports", Activity.MODE_PRIVATE)

  private fun tree(): Uri? = prefs().getString(TREE, null)?.let(Uri::parse)

  /** The picked folder as the person knows it ("Syncthing/ficelle"), empty without one. */
  @JavascriptInterface
  fun folder(): String {
    val tree = tree() ?: return ""
    val id = DocumentsContract.getTreeDocumentId(tree)
    return id.substringAfter(':').ifEmpty { id }
  }

  /** Opens the system's folder picker; the page hears back through __appReportFolder. */
  @JavascriptInterface
  fun pickFolder() {
    activity.runOnUiThread(pick)
  }

  /** Called with the picker's answer, null when the person backed out. */
  fun picked(uri: Uri?) {
    if (uri != null) {
      val flags =
        android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or
          android.content.Intent.FLAG_GRANT_WRITE_URI_PERMISSION
      activity.contentResolver.takePersistableUriPermission(uri, flags)
      prefs().edit().putString(TREE, uri.toString()).apply()
    }
    webView.evaluateJavascript("window.__appReportFolder?.()", null)
  }

  /**
   * Makes the report's folder with its report.md. Answers JSON: name and
   * path, or error.
   */
  @JavascriptInterface
  fun create(name: String, markdown: String): String =
    answer {
      val tree = tree() ?: throw IllegalStateException("no reports folder chosen")
      val root =
        DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
      val resolver = activity.contentResolver
      val dir =
        DocumentsContract.createDocument(
          resolver, root, DocumentsContract.Document.MIME_TYPE_DIR, name,
        ) ?: throw IllegalStateException("the folder refused a new report")
      // octet-stream: a text type would let the provider change the extension
      val file =
        DocumentsContract.createDocument(resolver, dir, "application/octet-stream", "report.md")
          ?: throw IllegalStateException("could not write report.md")
      resolver.openOutputStream(file)!!.use { it.write(markdown.toByteArray()) }
      made[name] = dir
      JSONObject().put("name", name).put("path", "${folder()}/$name")
    }

  /** Adds one image, base64, to a report made this run. Answers JSON, error when it failed. */
  @JavascriptInterface
  fun addImage(report: String, file: String, base64: String): String =
    answer {
      val dir = made[report] ?: throw IllegalStateException("no such report")
      val resolver = activity.contentResolver
      val doc =
        DocumentsContract.createDocument(resolver, dir, "image/png", file)
          ?: throw IllegalStateException("could not write $file")
      resolver.openOutputStream(doc)!!.use { it.write(Base64.decode(base64, Base64.DEFAULT)) }
      JSONObject()
    }

  /** The page as it shows now, a PNG in base64, empty when it failed. */
  @JavascriptInterface
  fun screenshot(): String {
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

  private fun answer(work: () -> JSONObject): String =
    try {
      work().toString()
    } catch (e: Exception) {
      JSONObject().put("error", e.message ?: e.toString()).toString()
    }

  companion object {
    private const val TREE = "tree"
  }
}
