package net.g2806.nebulahost

import android.os.Build
import android.os.Bundle
import android.system.Os
import android.util.Log
import androidx.activity.enableEdgeToEdge
import java.io.File
import kotlin.concurrent.thread

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    // The Rust side reads these when it starts, so they come before super.
    Os.setenv("NEBULA_NATIVE_LIB_DIR", applicationInfo.nativeLibraryDir, true)
    Os.setenv("NEBULA_FILES_DIR", filesDir.absolutePath, true)
    super.onCreate(savedInstanceState)
    installJava()
  }

  /**
   * Copy the Java runtimes that ship in the assets to the app's private folder
   * the first time (and again after an update). Android will not run a runtime
   * from the package itself, and the files are far too many to ship as
   * libraries, so they are plain assets that are copied here and started
   * through the launcher library (see nebula_java.c).
   *
   * Written beside the final folder and renamed when complete, so the app
   * never finds a half-copied runtime.
   */
  private fun installJava() {
    val abi = Build.SUPPORTED_ABIS.firstOrNull { assets.list("jre-$it")?.isNotEmpty() == true } ?: return
    val source = "jre-$abi"
    val target = File(filesDir, "nebula-host/runtimes/android-jre")
    val stamp = File(target, ".installed")
    val version = packageManager.getPackageInfo(packageName, 0).let {
      "${it.lastUpdateTime}-$abi"
    }
    if (stamp.exists() && stamp.readText() == version) return

    thread(name = "nebula-java-install") {
      try {
        val staging = File(target.parentFile, "android-jre.installing")
        staging.deleteRecursively()
        staging.mkdirs()
        val names = assets.open("$source/files.txt").bufferedReader().readLines().filter { it.isNotBlank() }
        for (name in names) {
          val out = File(staging, name)
          out.parentFile?.mkdirs()
          assets.open("$source/$name").use { input -> out.outputStream().use { input.copyTo(it) } }
        }
        File(staging, ".installed").writeText(version)
        target.deleteRecursively()
        if (!staging.renameTo(target)) throw IllegalStateException("could not move the runtime into place")
        Log.i("NebulaJava", "installed ${names.size} files for $abi")
      } catch (error: Exception) {
        Log.e("NebulaJava", "could not install the Java runtimes", error)
      }
    }
  }
}
