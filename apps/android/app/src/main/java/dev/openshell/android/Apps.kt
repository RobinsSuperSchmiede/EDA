package dev.openshell.android

import android.content.Context
import android.net.Uri
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/** Installed OpenShell apps: bundles unpacked into the app's private storage. */
object Apps {
    fun appsDir(ctx: Context) = File(ctx.filesDir, "apps").apply { mkdirs() }

    /** State of one app (databases, logs, pairing tokens) – kept when the app is updated. */
    fun stateDir(ctx: Context, identifier: String) = File(ctx.filesDir, "state/$identifier").apply { mkdirs() }

    fun list(ctx: Context): JSONArray =
        JSONObject(NativeRuntime.listApps(appsDir(ctx).absolutePath)).optJSONArray("apps") ?: JSONArray()

    fun find(ctx: Context, identifier: String): JSONObject? {
        val apps = list(ctx)
        for (i in 0 until apps.length()) {
            val a = apps.getJSONObject(i)
            if (a.optString("identifier") == identifier) return a
        }
        return null
    }

    /** Copy a bundle from a content URI (file picker, share sheet) and install it. */
    fun import(ctx: Context, uri: Uri): JSONObject {
        val tmp = File(ctx.cacheDir, "import.osapp")
        ctx.contentResolver.openInputStream(uri).use { input ->
            requireNotNull(input) { "cannot read $uri" }
            tmp.outputStream().use { input.copyTo(it) }
        }
        try {
            return JSONObject(NativeRuntime.installBundle(tmp.absolutePath, appsDir(ctx).absolutePath))
        } finally {
            tmp.delete()
        }
    }
}
