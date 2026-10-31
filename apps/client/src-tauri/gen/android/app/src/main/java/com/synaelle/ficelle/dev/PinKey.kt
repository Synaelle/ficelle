package com.synaelle.ficelle.dev

import android.app.Activity
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.security.KeyStore
import javax.crypto.KeyGenerator
import javax.crypto.Mac
import javax.crypto.SecretKey

// The PIN's hardware half. An HMAC key made in
// the Android Keystore, in StrongBox when the phone has one, never leaves it:
// the PIN goes through it before Argon2 (core vault.rs), so guessing a PIN
// needs this phone, not a copy of its files. Called from Rust through the
// plugin below (android_keystore.rs); the page has no permission for it.
object PinKey {
  private const val ALIAS = "ficelle-pin"

  @JvmStatic
  fun mix(data: ByteArray): ByteArray {
    val mac = Mac.getInstance("HmacSHA256")
    mac.init(key())
    return mac.doFinal(data)
  }

  @Synchronized
  private fun key(): SecretKey {
    val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
    if (Build.VERSION.SDK_INT >= 28) {
      try {
        return make(strongBox = true)
      } catch (_: StrongBoxUnavailableException) {
        // no StrongBox on this phone, the Keystore's own hardware then
      }
    }
    return make(strongBox = false)
  }

  private fun make(strongBox: Boolean): SecretKey {
    val spec = KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_SIGN).apply {
      if (strongBox && Build.VERSION.SDK_INT >= 28) setIsStrongBoxBacked(true)
    }.build()
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_HMAC_SHA256, "AndroidKeyStore")
    generator.init(spec)
    return generator.generateKey()
  }
}

@InvokeArg
class MixArgs {
  lateinit var data: String
}

/** [PinKey] for the Rust side, as the in-app plugin `pin-key`. */
@TauriPlugin
class PinKeyPlugin(activity: Activity) : Plugin(activity) {
  @Command
  fun mix(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(MixArgs::class.java)
      val mixed = PinKey.mix(Base64.decode(args.data, Base64.NO_WRAP))
      invoke.resolve(JSObject().put("data", Base64.encodeToString(mixed, Base64.NO_WRAP)))
    } catch (e: Exception) {
      invoke.reject(e.message ?: "the PIN key did not answer")
    }
  }
}
