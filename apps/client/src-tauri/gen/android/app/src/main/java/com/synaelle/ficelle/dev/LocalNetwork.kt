package com.synaelle.ficelle.dev

import android.app.Activity
import android.content.Context
import android.net.wifi.WifiManager
import android.os.Build
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class AskArgs {
  var ask: Boolean = false
}

/**
 * What local sync needs from Android, as the in-app plugin `local-network`
 * (sync/local.rs): the multicast lock, without which Wi-Fi drops the mDNS
 * packets devices find each other with, and from Android 17 the permission
 * to reach the local network at all. The page has no permission for this
 * plugin.
 */
@TauriPlugin(
  permissions = [
    Permission(strings = ["android.permission.ACCESS_LOCAL_NETWORK"], alias = "localNetwork"),
  ],
)
class LocalNetworkPlugin(private val activity: Activity) : Plugin(activity) {
  private var lock: WifiManager.MulticastLock? = null

  @Command
  fun acquireMulticast(invoke: Invoke) {
    val held = lock ?: run {
      val wifi = activity.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
      wifi.createMulticastLock("local sync").also {
        it.setReferenceCounted(false)
        lock = it
      }
    }
    if (!held.isHeld) held.acquire()
    invoke.resolve()
  }

  @Command
  fun releaseMulticast(invoke: Invoke) {
    lock?.let { if (it.isHeld) it.release() }
    invoke.resolve()
  }

  /** `granted`, `denied`, `prompt`; with `ask`, the system asks first. */
  @Command
  fun permission(invoke: Invoke) {
    val args = invoke.parseArgs(AskArgs::class.java)
    // before Android 17 the local network needs no permission
    if (Build.VERSION.SDK_INT < 37) {
      invoke.resolve(JSObject().put("state", PermissionState.GRANTED.toString()))
      return
    }
    val state = getPermissionState("localNetwork")
    if (state == PermissionState.GRANTED || !args.ask) {
      invoke.resolve(JSObject().put("state", state.toString()))
      return
    }
    requestPermissionForAlias("localNetwork", invoke, "answered")
  }

  @PermissionCallback
  fun answered(invoke: Invoke) {
    invoke.resolve(JSObject().put("state", getPermissionState("localNetwork").toString()))
  }

  /** The phone's model, the device name until the person gives one. */
  @Command
  fun model(invoke: Invoke) {
    invoke.resolve(JSObject().put("model", Build.MODEL))
  }
}
