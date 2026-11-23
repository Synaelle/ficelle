package com.synaelle.ficelle.dev

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Parcelable
import android.provider.OpenableColumns
import android.util.Log
import android.webkit.JavascriptInterface
import android.webkit.MimeTypeMap
import androidx.core.content.pm.ShortcutInfoCompat
import androidx.core.content.pm.ShortcutManagerCompat
import androidx.core.graphics.drawable.IconCompat
import java.io.File
import org.json.JSONArray
import org.json.JSONObject

/** The largest file a share copies in, 512 MB. */
private const val MAX_SHARED_BYTES = 512L * 1024 * 1024

/** Ends the name of a shared file while it is being copied in. */
private const val COPYING = ".part"

/** Leftovers are looked for once a process, before any share of its own. */
@Volatile private var leftoversTaken = false

// Shares from other apps, the icon's quick actions and taps on the home
// screen widget arrive as intents, often before the page is loaded or the
// notes are unlocked. They wait here until the page takes them
// (lib/inbox.ts). Shared files are copied into the app's cache right away,
// the other app's permission to read them ends with its intent. A copy the
// page never took, the app killed first, becomes a share again at the next
// start.
class Inbox(private val activity: Activity) {
  private val items = JSONArray()

  /** Tells the page something arrived, set once the webview exists. */
  var onArrival: (() -> Unit)? = null

  // File viewer: files opened with the app ("open with"), kept apart: the page shows them
  // even with the notes locked, and reads them in place, never copied in.
  private val files = JSONArray()

  /** Tells the page a file to open arrived. */
  var onFile: (() -> Unit)? = null

  private val newNote = "${activity.packageName}.NEW_NOTE"
  private val search = "${activity.packageName}.SEARCH"
  private val openNote = "${activity.packageName}.OPEN_NOTE"

  fun receive(intent: Intent?) {
    when (intent?.action) {
      Intent.ACTION_SEND, Intent.ACTION_SEND_MULTIPLE -> share(intent)
      Intent.ACTION_VIEW -> intent.data?.takeIf { foreign(it) }?.let { uri ->
        synchronized(files) { files.put(uri.toString()) }
        activity.runOnUiThread { onFile?.invoke() }
      }
      newNote -> add(JSONObject().put("kind", "newNote"))
      search -> add(JSONObject().put("kind", "search"))
      openNote -> intent.getStringExtra("noteId")?.let {
        add(JSONObject().put("kind", "openNote").put("id", it))
      }
    }
  }

  /**
   * The shared files a killed app left in the inbox, each waiting again as
   * a share of its own, and the copies it was cut off in removed. Call it
   * before the launch intent is received.
   */
  fun takeLeftovers() {
    if (leftoversTaken) return
    leftoversTaken = true
    val left = File(activity.cacheDir, "inbox").listFiles() ?: return
    for (file in left.sortedBy { it.name }) {
      if (file.name.endsWith(COPYING)) {
        file.delete()
        continue
      }
      // copies are named <nanoTime>-<name>
      val name = file.name.substringAfter('-')
      val mime = MimeTypeMap.getSingleton()
        .getMimeTypeFromExtension(name.substringAfterLast('.', "").lowercase())
        ?: "application/octet-stream"
      val files = JSONArray().put(
        JSONObject().put("path", file.absolutePath).put("name", name).put("mime", mime),
      )
      add(JSONObject().put("kind", "share").put("text", "").put("subject", "").put("files", files))
    }
  }

  /** Everything waiting, as a json array, and forgets it. */
  @JavascriptInterface
  fun take(): String = synchronized(items) {
    val out = items.toString()
    while (items.length() > 0) items.remove(0)
    out
  }

  /** The addresses of the files waiting to open, as a json array, and forgets them. */
  @JavascriptInterface
  fun takeFiles(): String = synchronized(files) {
    val out = files.toString()
    while (files.length() > 0) files.remove(0)
    out
  }

  /** Long press on the app icon: new note and search. */
  fun publishShortcuts() {
    val shortcuts = listOf(
      shortcut("new_note", R.string.shortcut_new_note, R.drawable.ic_shortcut_new_note, newNote),
      shortcut("search", R.string.shortcut_search, R.drawable.ic_shortcut_search, search),
    )
    ShortcutManagerCompat.setDynamicShortcuts(activity, shortcuts)
  }

  private fun shortcut(id: String, label: Int, icon: Int, action: String) =
    ShortcutInfoCompat.Builder(activity, id)
      .setShortLabel(activity.getString(label))
      .setIcon(IconCompat.createWithResource(activity, icon))
      .setIntent(Intent(activity, MainActivity::class.java).setAction(action))
      .build()

  private fun add(item: JSONObject) {
    synchronized(items) { items.put(item) }
    activity.runOnUiThread { onArrival?.invoke() }
  }

  private fun share(intent: Intent) {
    val text = intent.getStringExtra(Intent.EXTRA_TEXT) ?: ""
    val subject = intent.getStringExtra(Intent.EXTRA_SUBJECT) ?: ""
    val uris = streams(intent)
    // copying a large file must not freeze the screen
    Thread {
      val files = JSONArray()
      for (uri in uris) copy(uri, intent.type)?.let { files.put(it) }
      add(
        JSONObject()
          .put("kind", "share")
          .put("text", text)
          .put("subject", subject)
          .put("files", files),
      )
    }.start()
  }

  @Suppress("DEPRECATION")
  private fun streams(intent: Intent): List<Uri> =
    if (intent.action == Intent.ACTION_SEND_MULTIPLE) {
      if (Build.VERSION.SDK_INT >= 33) {
        intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
      } else {
        intent.getParcelableArrayListExtra<Parcelable>(Intent.EXTRA_STREAM)?.filterIsInstance<Uri>()
      } ?: emptyList()
    } else {
      val uri = if (Build.VERSION.SDK_INT >= 33) {
        intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
      } else {
        intent.getParcelableExtra<Parcelable>(Intent.EXTRA_STREAM) as? Uri
      }
      listOfNotNull(uri)
    }

  // Only another app's content: a file:// path or this app's own provider
  // would let any app have Ficelle copy its private files (the vault, its
  // keys) into a note.
  private fun foreign(uri: Uri): Boolean {
    val authority = uri.authority ?: return false
    return uri.scheme == "content" &&
      authority != activity.packageName &&
      !authority.startsWith("${activity.packageName}.")
  }

  private fun copy(uri: Uri, fallbackType: String?): JSONObject? = try {
    if (!foreign(uri)) throw SecurityException("not another app's file: $uri")
    val resolver = activity.contentResolver
    val name = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
      ?.use { if (it.moveToFirst()) it.getString(0) else null }
      ?: uri.lastPathSegment
      ?: "file"
    // the name only labels the copy, a slash or dots must not leave the folder
    val safe = name.replace(Regex("[^A-Za-z0-9._ -]"), "_").trimStart('.')
    val dir = File(activity.cacheDir, "inbox").apply { mkdirs() }
    val file = File(dir, "${System.nanoTime()}-$safe")
    val partial = File(dir, file.name + COPYING)
    val copied = resolver.openInputStream(uri)?.use { input ->
      partial.outputStream().use { output ->
        // a cap, so a share cannot fill the phone's storage
        val buffer = ByteArray(64 * 1024)
        var total = 0L
        while (true) {
          val read = input.read(buffer)
          if (read < 0) break
          total += read
          if (total > MAX_SHARED_BYTES) {
            output.close()
            partial.delete()
            throw java.io.IOException("a shared file over ${MAX_SHARED_BYTES / (1024 * 1024)} MB")
          }
          output.write(buffer, 0, read)
        }
        total
      }
    }
    if (copied == null) {
      partial.delete()
      null
    } else if (!partial.renameTo(file)) {
      partial.delete()
      throw java.io.IOException("could not name the copy of a shared file")
    } else {
      JSONObject()
        .put("path", file.absolutePath)
        .put("name", name)
        .put("mime", resolver.getType(uri) ?: fallbackType ?: "application/octet-stream")
    }
  } catch (e: Exception) {
    Log.w("Inbox", "could not copy a shared file", e)
    null
  }
}
