package com.synaelle.ficelle.dev

import android.app.Activity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.text.format.Formatter
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class ShowArgs {
  lateinit var name: String
  var done: Long = 0
  var total: Long = 0
}

/**
 * A download the person asked for, as the in-app plugin `file-transfer`
 * (android_transfer.rs): its notification shows how far it got, and the
 * foreground service behind it keeps the app running until the file is
 * here. Without the notification permission (Android 13 and later) the
 * service still runs, the system lists it in its running apps only. The
 * page has no permission for this plugin.
 */
@TauriPlugin
class TransferPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun show(invoke: Invoke) {
    val args = invoke.parseArgs(ShowArgs::class.java)
    val context = activity.applicationContext
    TransferService.show(context, args.name, args.done, args.total)
    invoke.resolve()
  }

  @Command
  fun stop(invoke: Invoke) {
    val context = activity.applicationContext
    context.stopService(Intent(context, TransferService::class.java))
    invoke.resolve()
  }
}

class TransferService : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val notification = notification(
      this,
      intent?.getStringExtra(NAME) ?: "",
      intent?.getLongExtra(DONE, 0) ?: 0,
      intent?.getLongExtra(TOTAL, 0) ?: 0,
    )
    if (Build.VERSION.SDK_INT >= 29) {
      startForeground(ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
    } else {
      startForeground(ID, notification)
    }
    running = true
    // not started again by the system: the download ended with the process
    return START_NOT_STICKY
  }

  override fun onDestroy() {
    running = false
    super.onDestroy()
  }

  // Android 15 gives data sync services a few hours a day: past them the
  // download goes on only while the app is in front
  override fun onTimeout(startId: Int, fgsType: Int) {
    stopSelf()
  }

  companion object {
    private const val CHANNEL = "downloads"
    private const val ID = 7301
    private const val NAME = "name"
    private const val DONE = "done"
    private const val TOTAL = "total"

    @Volatile
    private var running = false

    fun show(context: Context, name: String, done: Long, total: Long) {
      if (running) {
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.notify(ID, notification(context, name, done, total))
        return
      }
      val intent = Intent(context, TransferService::class.java)
        .putExtra(NAME, name)
        .putExtra(DONE, done)
        .putExtra(TOTAL, total)
      try {
        ContextCompat.startForegroundService(context, intent)
      } catch (_: IllegalStateException) {
        // started from the background (Android 12 and later refuse): the
        // download goes on while the app runs, without its notification
      }
    }

    private fun notification(context: Context, name: String, done: Long, total: Long): Notification {
      val manager = context.getSystemService(NotificationManager::class.java)
      if (Build.VERSION.SDK_INT >= 26 && manager.getNotificationChannel(CHANNEL) == null) {
        manager.createNotificationChannel(
          NotificationChannel(
            CHANNEL,
            context.getString(R.string.downloads_channel),
            NotificationManager.IMPORTANCE_LOW,
          ),
        )
      }
      val open = PendingIntent.getActivity(
        context,
        0,
        Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
      )
      // sizes in the phone's own units, with no words to translate
      val sizes = "${Formatter.formatShortFileSize(context, done)} / " +
        Formatter.formatShortFileSize(context, total)
      val percent = if (total > 0) (done * 100 / total).toInt() else 0
      return NotificationCompat.Builder(context, CHANNEL)
        .setSmallIcon(R.drawable.ic_notification)
        .setContentTitle(name)
        .setContentText(sizes)
        .setProgress(100, percent, total <= 0)
        .setContentIntent(open)
        .setOngoing(true)
        .setOnlyAlertOnce(true)
        .setSilent(true)
        .setCategory(NotificationCompat.CATEGORY_PROGRESS)
        .build()
    }
  }
}
