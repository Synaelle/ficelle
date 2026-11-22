package com.synaelle.ficelle.dev

import android.app.Activity
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.WindowManager
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.content.FileProvider
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import java.io.File

class MainActivity : TauriActivity() {
  // The page has no browser history to go back through. It closes its own
  // layers (a note, settings, a dialog) and says whether it did, and only
  // when nothing was open does back leave the app.
  override val handleBackNavigation = false

  private val inbox by lazy { Inbox(this) }

  // dev and preview builds only, see DevReports
  private var devReports: DevReports? = null
  private val reportFolderPicker =
    registerForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
      devReports?.picked(uri)
    }

  override fun onWebViewCreate(webView: WebView) {
    webView.addJavascriptInterface(inbox, "AppInbox")
    webView.addJavascriptInterface(WidgetBridge(applicationContext), "AppWidget")
    webView.addJavascriptInterface(SystemInfo(this), "AppSystem")
    webView.addJavascriptInterface(Installer(this), "AppInstaller")
    webView.addJavascriptInterface(Screenshot(this, webView), "AppShot")
    if (BuildConfig.DEV_REPORTS) {
      val reports = DevReports(this, webView) { reportFolderPicker.launch(null) }
      devReports = reports
      webView.addJavascriptInterface(reports, "AppReports")
    }
    // The webview would grow text alone with the system font size and leave
    // rows and buttons at their size. The page scales as a whole instead,
    // from the same setting (lib/uiSize.ts).
    webView.settings.textZoom = 100
    inbox.onArrival = { webView.evaluateJavascript("window.__appInbox?.()", null) }
    // file viewer
    inbox.onFile = { webView.evaluateJavascript("window.__appFile?.()", null) }
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        webView.evaluateJavascript("window.__appBack?.() ?? false") { handled ->
          if (handled != "true") {
            isEnabled = false
            onBackPressedDispatcher.onBackPressed()
            isEnabled = true
          }
        }
      }
    })
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    SystemInfo.applyHidden(this)
    // a recreated activity already handed its launch intent over
    if (savedInstanceState == null) inbox.receive(intent)
    inbox.publishShortcuts()
    NotesWidget.refresh(this)
    // Edge to edge apps draw under the status bar, the navigation bar and the
    // keyboard. Padding the content by them keeps the page clear of the bars
    // and makes it shrink when the keyboard opens, so the formatting bar at
    // the bottom of the page rides on top of the keyboard.
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val bars = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout(),
      )
      val keyboard = insets.getInsets(WindowInsetsCompat.Type.ime())
      view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, keyboard.bottom))
      WindowInsetsCompat.CONSUMED
    }
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    inbox.receive(intent)
  }
}

/** What the page reads from the system, as window.AppSystem. */
class SystemInfo(private val activity: Activity) {
  /**
   * The font size chosen in Android's settings, 1 by default. A change
   * recreates the activity, so the page reads it again on load.
   */
  @JavascriptInterface
  fun fontScale(): Float = activity.resources.configuration.fontScale

  /**
   * Where backups go on this phone, created if needed: "Ficelle backups" in
   * the shared Documents from Android 11 (no permission needed, a file
   * manager or a computer over USB sees it, it stays when the app goes),
   * the app's own storage before.
   */
  @JavascriptInterface
  fun backupFolder(): String {
    val name = "${activity.getString(R.string.app_name)} backups"
    val dir =
      if (Build.VERSION.SDK_INT >= 30) {
        java.io.File(
          android.os.Environment.getExternalStoragePublicDirectory(
            android.os.Environment.DIRECTORY_DOCUMENTS
          ),
          name,
        )
      } else {
        java.io.File(activity.getExternalFilesDir(null) ?: activity.filesDir, name)
      }
    dir.mkdirs()
    return dir.absolutePath
  }

  /** Opens a web page in the phone's browser: the sync server's sign-in. */
  @JavascriptInterface
  fun openBrowser(url: String): Boolean {
    val uri = android.net.Uri.parse(url)
    if (uri.scheme != "https" && uri.scheme != "http") return false
    // a new tab: the browser would bring back the one it had for this app
    val intent = Intent(Intent.ACTION_VIEW, uri)
      .addCategory(Intent.CATEGORY_BROWSABLE)
      .putExtra(android.provider.Browser.EXTRA_CREATE_NEW_TAB, true)
    return try {
      activity.runOnUiThread { activity.startActivity(intent) }
      true
    } catch (e: android.content.ActivityNotFoundException) {
      false
    }
  }

  /**
   * Opens an attachment in the phone's app for its type. The Rust side
   * writes a copy into cache/opened just before (sync/files.rs), so the
   * page passes no path: the newest copy there is the one. With no app for
   * the type, the system's chooser says so.
   */
  @JavascriptInterface
  fun openFile(): Boolean {
    val file = File(activity.cacheDir, "opened")
      .listFiles()
      ?.flatMap { it.listFiles()?.toList() ?: emptyList() }
      ?.maxByOrNull { it.lastModified() }
      ?: return false
    val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", file)
    val intent = Intent(Intent.ACTION_VIEW)
      .setDataAndType(uri, activity.contentResolver.getType(uri))
      .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    activity.runOnUiThread {
      try {
        activity.startActivity(intent)
      } catch (e: android.content.ActivityNotFoundException) {
        activity.startActivity(Intent.createChooser(intent, null))
      }
    }
    return true
  }

  /** Whether the phone shows a 24-hour clock, for times in the app to match. */
  @JavascriptInterface
  fun clock24h(): Boolean = android.text.format.DateFormat.is24HourFormat(activity)

  /** Whether notes stay out of screenshots and the recent apps. */
  @JavascriptInterface
  fun hidden(): Boolean = prefs(activity).getBoolean(HIDDEN, false)

  /** Kept here, not in the page, so it applies from the first screen on. */
  @JavascriptInterface
  fun setHidden(on: Boolean) {
    prefs(activity).edit().putBoolean(HIDDEN, on).apply()
    activity.runOnUiThread { applyHidden(activity) }
  }

  /**
   * The page's background behind the status and navigation bars, which the
   * content is padded away from, and icons that show on it.
   */
  @JavascriptInterface
  fun setBars(color: String, light: Boolean) {
    val parsed = try {
      android.graphics.Color.parseColor(color)
    } catch (_: IllegalArgumentException) {
      return
    }
    activity.runOnUiThread {
      val window = activity.window
      window.decorView.setBackgroundColor(parsed)
      WindowCompat.getInsetsController(window, window.decorView).apply {
        isAppearanceLightStatusBars = light
        isAppearanceLightNavigationBars = light
      }
    }
  }

  companion object {
    private const val HIDDEN = "hideFromScreenshots"

    private fun prefs(activity: Activity) =
      activity.getSharedPreferences("system", Activity.MODE_PRIVATE)

    // FLAG_SECURE: screenshots and screen recordings come out black, the
    // recent apps show a blank card
    fun applyHidden(activity: Activity) {
      if (prefs(activity).getBoolean(HIDDEN, false)) {
        activity.window.setFlags(
          WindowManager.LayoutParams.FLAG_SECURE,
          WindowManager.LayoutParams.FLAG_SECURE,
        )
      } else {
        activity.window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
      }
    }
  }
}

/**
 * Hands the downloaded update to the system installer, as window.AppInstaller.
 * The app's Rust side downloads it into cache/updates and only names it
 * there once its signature checks out (commands/updates.rs), so the page
 * passes no path. Android then asks the person, and the first time sends
 * them to allow installs from this app.
 */
class Installer(private val activity: Activity) {
  /**
   * The name of the app store that installed this app, or last updated it:
   * F-Droid, the apps that read F-Droid repositories, Obtainium, Google
   * Play. That store updates it, so the app's own updater stays out of the
   * way. Empty for any other: installed from a browser or a file manager,
   * the system's installer is named, or nobody.
   */
  @JavascriptInterface
  fun store(): String {
    val installer = try {
      if (Build.VERSION.SDK_INT >= 30) {
        activity.packageManager.getInstallSourceInfo(activity.packageName).installingPackageName
      } else {
        @Suppress("DEPRECATION")
        activity.packageManager.getInstallerPackageName(activity.packageName)
      }
    } catch (e: android.content.pm.PackageManager.NameNotFoundException) {
      null
    }
    return STORES[installer] ?: ""
  }

  /** The connection is metered (mobile data, a phone's hotspot): a download asks first. */
  @JavascriptInterface
  fun metered(): Boolean =
    activity.getSystemService(android.net.ConnectivityManager::class.java)?.isActiveNetworkMetered ?: false

  @JavascriptInterface
  fun install(): Boolean {
    val apk = File(activity.cacheDir, "updates")
      .listFiles { file -> file.name.endsWith(".apk") }
      ?.maxByOrNull { it.lastModified() }
      ?: return false
    val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", apk)
    val intent = Intent(Intent.ACTION_VIEW)
      .setDataAndType(uri, "application/vnd.android.package-archive")
      .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
    activity.runOnUiThread { activity.startActivity(intent) }
    return true
  }

  /**
   * Going back from a beta to Stable: Android installs no older version over
   * a newer one, so the checked stable APK (`version`, kept in cache/updates
   * by commands/updates.rs) is copied to the shared Downloads, where it stays
   * once this app is uninstalled. Returns where it went (`Download/Ficelle-0.4.1.apk`),
   * empty when it could not: before Android 10 the shared Downloads needs a
   * permission the app does not ask for.
   */
  @JavascriptInterface
  fun saveToDownloads(version: String): String {
    if (Build.VERSION.SDK_INT < 29) return ""
    if (!Regex("""^[0-9]+\.[0-9]+\.[0-9]+$""").matches(version)) return ""
    val apk = File(activity.cacheDir, "updates")
      .listFiles { file -> file.name.endsWith("-$version.apk") }
      ?.firstOrNull()
      ?: return ""
    val name = "${activity.getString(R.string.app_name).replace(' ', '-')}-$version.apk"
    val resolver = activity.contentResolver
    // kept outside the try: an entry left pending after a failure is removed
    var pending: android.net.Uri? = null
    return try {
      val values = android.content.ContentValues().apply {
        put(android.provider.MediaStore.Downloads.DISPLAY_NAME, name)
        put(android.provider.MediaStore.Downloads.MIME_TYPE, "application/vnd.android.package-archive")
        put(android.provider.MediaStore.Downloads.IS_PENDING, 1)
      }
      val uri =
        resolver.insert(android.provider.MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
          ?: return ""
      pending = uri
      val copied = resolver.openOutputStream(uri)?.use { out ->
        apk.inputStream().use { it.copyTo(out) }
      }
      if (copied == null) {
        resolver.delete(uri, null, null)
        return ""
      }
      values.clear()
      values.put(android.provider.MediaStore.Downloads.IS_PENDING, 0)
      resolver.update(uri, values, null, null)
      pending = null
      // the name Android gave it, "(1)" added when one was there already
      val saved = resolver.query(
        uri,
        arrayOf(android.provider.MediaStore.Downloads.DISPLAY_NAME),
        null,
        null,
        null,
      )?.use { if (it.moveToFirst()) it.getString(0) else null } ?: name
      "${android.os.Environment.DIRECTORY_DOWNLOADS}/$saved"
    } catch (e: Exception) {
      android.util.Log.w("Ficelle", "saving the update to Downloads failed", e)
      pending?.let { resolver.delete(it, null, null) }
      ""
    }
  }

  /**
   * The system's screen to uninstall this app, which asks the person. App
   * info, which has an Uninstall button, if that screen does not open.
   */
  @JavascriptInterface
  fun uninstall(): Boolean {
    val uri = android.net.Uri.parse("package:${activity.packageName}")
    activity.runOnUiThread {
      try {
        activity.startActivity(
          Intent(Intent.ACTION_DELETE, uri).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        )
      } catch (e: Exception) {
        activity.startActivity(
          Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS, uri)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        )
      }
    }
    return true
  }

  /** Backups stay when the app is uninstalled: they go to the shared
   * Documents from Android 11 (SystemInfo.backupFolder). */
  @JavascriptInterface
  fun backupsKept(): Boolean = Build.VERSION.SDK_INT >= 30
}

/** The stores that update the app themselves, by the name the app shows, see [Installer.store]. */
private val STORES = mapOf(
  "org.fdroid.fdroid" to "F-Droid",
  "org.fdroid.basic" to "F-Droid",
  "org.fdroid.fdroid.privileged" to "F-Droid",
  "com.looker.droidify" to "F-Droid",
  "com.machiav3lli.fdroid" to "F-Droid",
  "dev.imranr.obtainium" to "Obtainium",
  "dev.imranr.obtainium.fdroid" to "Obtainium",
  "com.android.vending" to "Google Play",
)
