package com.synaelle.ficelle.dev

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.DocumentsContract.Document
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class DocArgs {
  lateinit var tree: String
  lateinit var path: String
  var name: String = ""
}

/**
 * A backups folder reached through the system's document access, as the
 * in-app plugin `backup-folder` (documents.rs): Android lets a new install
 * read none of the files an earlier install made in Documents, until the
 * person picks the folder once in the system's picker. Paths are relative to
 * the picked folder, with `/`. The files themselves are opened by their
 * address through the fs plugin. The page has no permission for this plugin.
 */
@TauriPlugin
class BackupFolderPlugin(private val activity: Activity) : Plugin(activity) {
  private val resolver get() = activity.contentResolver

  /** Opens the folder picker on the phone's backups folder (SystemInfo.backupFolder). */
  @Command
  fun pick(invoke: Invoke) {
    val name = "${activity.getString(R.string.app_name)} backups"
    val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).addFlags(
      Intent.FLAG_GRANT_READ_URI_PERMISSION or
        Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
        Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION,
    )
    if (Build.VERSION.SDK_INT >= 26) {
      intent.putExtra(
        DocumentsContract.EXTRA_INITIAL_URI,
        DocumentsContract.buildDocumentUri(
          EXTERNAL,
          "primary:${Environment.DIRECTORY_DOCUMENTS}/$name",
        ),
      )
    }
    startActivityForResult(invoke, intent, "picked")
  }

  @ActivityCallback
  fun picked(invoke: Invoke, result: ActivityResult) {
    val tree = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || tree == null) {
      invoke.reject("cancelled")
      return
    }
    // kept across restarts, scheduled backups go on writing there
    resolver.takePersistableUriPermission(
      tree,
      Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
    )
    invoke.resolve(JSObject().put("tree", tree.toString()))
  }

  /** The file's address, none when it is not there. */
  @Command
  fun find(invoke: Invoke) = answer(invoke) { tree, args ->
    val doc = walk(tree, args.path, make = false)
    JSObject().put("uri", doc?.let { DocumentsContract.buildDocumentUriUsingTree(tree, it.id).toString() })
  }

  /** A new empty file, its folders made as needed, one of that name removed first. */
  @Command
  fun create(invoke: Invoke) = answer(invoke) { tree, args ->
    val (dir, name) = split(args.path)
    val parent = walk(tree, dir, make = true)!!
    child(tree, parent.id, name)?.let { delete(tree, it) }
    // octet-stream: any other type would let the provider add an extension
    val uri = DocumentsContract.createDocument(
      resolver,
      DocumentsContract.buildDocumentUriUsingTree(tree, parent.id),
      "application/octet-stream",
      name,
    ) ?: throw IllegalStateException("the folder refused $name")
    if (displayName(uri) != name) {
      DocumentsContract.deleteDocument(resolver, uri)
      throw IllegalStateException("the folder named $name otherwise")
    }
    JSObject().put("uri", uri.toString())
  }

  /** Gives the file another name in its folder. */
  @Command
  fun rename(invoke: Invoke) = answer(invoke) { tree, args ->
    val doc = walk(tree, args.path, make = false)
      ?: throw java.io.FileNotFoundException(args.path)
    val uri = DocumentsContract.renameDocument(
      resolver,
      DocumentsContract.buildDocumentUriUsingTree(tree, doc.id),
      args.name,
    ) ?: DocumentsContract.buildDocumentUriUsingTree(tree, doc.id)
    // a provider that finds the name taken picks another: never keep that
    if (displayName(uri) != args.name) {
      DocumentsContract.renameDocument(resolver, uri, split(args.path).second)
      throw IllegalStateException("${args.name} is taken")
    }
    JSObject().put("uri", uri.toString())
  }

  /** Removes the file. One that is not there is fine. */
  @Command
  fun delete(invoke: Invoke) = answer(invoke) { tree, args ->
    walk(tree, args.path, make = false)?.let { delete(tree, it) }
    JSObject()
  }

  /** Every file under the folder `path` ("" for the whole), at any depth, with its size. */
  @Command
  fun list(invoke: Invoke) = answer(invoke) { tree, args ->
    val files = JSArray()
    val start = walk(tree, args.path.trimEnd('/'), make = false)
    if (start != null && start.dir) {
      val prefix = args.path.trimEnd('/').let { if (it.isEmpty()) "" else "$it/" }
      collect(tree, start.id, prefix, files)
    }
    JSObject().put("files", files)
  }

  private class Doc(val id: String, val name: String, val dir: Boolean, val size: Long)

  private fun collect(tree: Uri, id: String, prefix: String, out: JSArray) {
    for (doc in children(tree, id)) {
      if (doc.dir) {
        collect(tree, doc.id, "$prefix${doc.name}/", out)
      } else if (!doc.name.endsWith(".partial") && !doc.name.endsWith(".old")) {
        // a write that never finished, or a file being replaced
        out.put(JSObject().put("path", "$prefix${doc.name}").put("size", doc.size))
      }
    }
  }

  private fun children(tree: Uri, id: String): List<Doc> {
    val columns = arrayOf(
      Document.COLUMN_DOCUMENT_ID,
      Document.COLUMN_DISPLAY_NAME,
      Document.COLUMN_MIME_TYPE,
      Document.COLUMN_SIZE,
    )
    val out = mutableListOf<Doc>()
    resolver.query(DocumentsContract.buildChildDocumentsUriUsingTree(tree, id), columns, null, null, null)
      ?.use { row ->
        while (row.moveToNext()) {
          out.add(
            Doc(
              row.getString(0),
              row.getString(1),
              row.getString(2) == Document.MIME_TYPE_DIR,
              if (row.isNull(3)) 0 else row.getLong(3),
            ),
          )
        }
      }
    return out
  }

  // providers ignore a query's selection, the name is matched here
  private fun child(tree: Uri, id: String, name: String): Doc? =
    children(tree, id).firstOrNull { it.name == name }

  /** The document at `path`, its folders made on the way when `make`. */
  private fun walk(tree: Uri, path: String, make: Boolean): Doc? {
    var doc = Doc(DocumentsContract.getTreeDocumentId(tree), "", true, 0)
    if (path.isEmpty()) return doc
    for (part in path.split('/')) {
      if (!doc.dir) return null
      doc = child(tree, doc.id, part) ?: if (make) {
        val made = DocumentsContract.createDocument(
          resolver,
          DocumentsContract.buildDocumentUriUsingTree(tree, doc.id),
          Document.MIME_TYPE_DIR,
          part,
        ) ?: throw IllegalStateException("the folder refused $part")
        Doc(DocumentsContract.getDocumentId(made), part, true, 0)
      } else {
        return null
      }
    }
    return doc
  }

  private fun delete(tree: Uri, doc: Doc) {
    DocumentsContract.deleteDocument(resolver, DocumentsContract.buildDocumentUriUsingTree(tree, doc.id))
  }

  private fun displayName(uri: Uri): String? =
    resolver.query(uri, arrayOf(Document.COLUMN_DISPLAY_NAME), null, null, null)
      ?.use { if (it.moveToFirst()) it.getString(0) else null }

  private fun split(path: String): Pair<String, String> =
    path.substringBeforeLast('/', "") to path.substringAfterLast('/')

  /**
   * Runs one call. A folder whose access was taken back answers "denied",
   * so the page can ask for it again.
   */
  private fun answer(invoke: Invoke, work: (Uri, DocArgs) -> JSObject) {
    try {
      val args = invoke.parseArgs(DocArgs::class.java)
      invoke.resolve(work(Uri.parse(args.tree), args))
    } catch (e: SecurityException) {
      invoke.reject("denied: ${e.message}")
    } catch (e: Exception) {
      invoke.reject(e.message ?: e.toString())
    }
  }

  companion object {
    private const val EXTERNAL = "com.android.externalstorage.documents"
  }
}
