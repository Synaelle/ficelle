package com.synaelle.ficelle.dev

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.view.View
import android.webkit.JavascriptInterface
import android.widget.RemoteViews
import org.json.JSONArray
import org.json.JSONObject

// Home screen widget: new note, search and up to four recent notes. It runs
// outside the app and cannot open the vault, so the page sends it a small
// snapshot through WidgetBridge whenever the list changes (lib/widget.ts).
// Titles of encrypted notes never leave the app, the page sends a message
// instead of them.
class NotesWidget : AppWidgetProvider() {
  override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
    render(context, manager, ids)
  }

  companion object {
    private const val PREFS = "widget"
    private const val SNAPSHOT = "snapshot"
    private val ROWS = intArrayOf(
      R.id.widget_note_0,
      R.id.widget_note_1,
      R.id.widget_note_2,
      R.id.widget_note_3,
    )

    fun save(context: Context, json: String) {
      context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(SNAPSHOT, json).apply()
      refresh(context)
    }

    /**
     * Draws every placed widget again. Clearing the app's data cancels the
     * taps a widget holds and Android does not redraw it, the app does on
     * each start.
     */
    fun refresh(context: Context) {
      val manager = AppWidgetManager.getInstance(context)
      val ids = manager.getAppWidgetIds(ComponentName(context, NotesWidget::class.java))
      if (ids.isNotEmpty()) render(context, manager, ids)
    }

    private fun render(context: Context, manager: AppWidgetManager, ids: IntArray) {
      val app = context.packageName
      val views = RemoteViews(app, R.layout.widget)
      views.setOnClickPendingIntent(R.id.widget_new_note, launch(context, 1, "$app.NEW_NOTE"))
      views.setOnClickPendingIntent(R.id.widget_search, launch(context, 2, "$app.SEARCH"))
      views.setOnClickPendingIntent(R.id.widget_title, launch(context, 3, Intent.ACTION_MAIN))

      val saved = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(SNAPSHOT, null)
      val snapshot = saved?.let { runCatching { JSONObject(it) }.getOrNull() }
      if (snapshot != null) {
        views.setTextViewText(R.id.widget_title, snapshot.optString("title"))
        val message = snapshot.optString("message")
        views.setTextViewText(R.id.widget_message, message)
        views.setViewVisibility(R.id.widget_message, if (message.isEmpty()) View.GONE else View.VISIBLE)
        val notes = snapshot.optJSONArray("notes") ?: JSONArray()
        ROWS.forEachIndexed { i, row ->
          val note = notes.optJSONObject(i)
          if (note == null) {
            views.setViewVisibility(row, View.GONE)
          } else {
            views.setViewVisibility(row, View.VISIBLE)
            views.setTextViewText(row, note.optString("title"))
            val open = launch(context, 10 + i, "$app.OPEN_NOTE", note.optString("id"))
            views.setOnClickPendingIntent(row, open)
          }
        }
      }
      manager.updateAppWidget(ids, views)
    }

    private fun launch(context: Context, code: Int, action: String, noteId: String? = null): PendingIntent {
      val intent = Intent(context, MainActivity::class.java)
        .setAction(action)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
      if (noteId != null) intent.putExtra("noteId", noteId)
      return PendingIntent.getActivity(
        context,
        code,
        intent,
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
      )
    }
  }
}

/** The page's way to refresh the widget, as window.AppWidget. */
class WidgetBridge(private val context: Context) {
  @JavascriptInterface
  fun update(json: String) = NotesWidget.save(context, json)
}
