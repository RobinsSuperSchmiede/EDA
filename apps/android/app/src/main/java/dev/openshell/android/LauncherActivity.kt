package dev.openshell.android

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.webkit.JavascriptInterface
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import org.json.JSONObject
import java.util.concurrent.Executors

/**
 * The app manager: installed OpenShell apps, importing bundles (.osapp) from files or the share
 * sheet, opening and stopping apps. Its UI is `assets/launcher.html`; only this WebView gets the
 * `OpenShellHost` interface, and it loads nothing but that page.
 */
class LauncherActivity : Activity() {
    private lateinit var web: WebView
    private val io = Executors.newSingleThreadExecutor()
    private val pickBundle = 1

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        web = WebView(this)
        web.settings.javaScriptEnabled = true
        web.webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                // the launcher never navigates away; external links open in the browser
                startActivity(Intent(Intent.ACTION_VIEW, request.url))
                return true
            }
        }
        web.addJavascriptInterface(Host(), "OpenShellHost")
        setContentView(web)
        web.loadUrl("file:///android_asset/launcher.html")

        if (Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 0)
        }
        handleIncoming(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIncoming(intent)
    }

    override fun onResume() {
        super.onResume()
        refresh()
    }

    /** A bundle opened with or shared to OpenShell. */
    private fun handleIncoming(intent: Intent?) {
        val uri: Uri? = when (intent?.action) {
            Intent.ACTION_VIEW -> intent.data
            Intent.ACTION_SEND -> if (Build.VERSION.SDK_INT >= 33) {
                intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                @Suppress("DEPRECATION")
                intent.getParcelableExtra(Intent.EXTRA_STREAM)
            }
            else -> null
        }
        if (uri != null) import(uri)
    }

    private fun import(uri: Uri) {
        io.execute {
            val r = runCatching { Apps.import(this, uri) }
                .getOrElse { JSONObject().put("ok", false).put("error", it.message ?: it.toString()) }
            runOnUiThread { notifyPage("onImported", r.toString()) }
        }
    }

    private fun refresh() = notifyPage("onAppsChanged", "null")

    private fun notifyPage(fn: String, json: String) {
        web.evaluateJavascript("window.$fn && window.$fn($json)", null)
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == pickBundle && resultCode == RESULT_OK) data?.data?.let { import(it) }
    }

    /** Called from launcher.html (on a binder thread). */
    inner class Host {
        @JavascriptInterface
        fun listApps(): String = JSONObject().put("apps", Apps.list(this@LauncherActivity)).toString()

        @JavascriptInterface
        fun status(): String = RuntimeService.status.toString()

        @JavascriptInterface
        fun openApp(identifier: String) {
            runOnUiThread {
                startActivity(
                    Intent(this@LauncherActivity, AppActivity::class.java)
                        .putExtra(AppActivity.EXTRA_IDENTIFIER, identifier)
                )
            }
        }

        @JavascriptInterface
        fun importBundle() {
            runOnUiThread {
                val pick = Intent(Intent.ACTION_OPEN_DOCUMENT)
                    .addCategory(Intent.CATEGORY_OPENABLE)
                    .setType("*/*")
                @Suppress("DEPRECATION")
                startActivityForResult(pick, pickBundle)
            }
        }

        @JavascriptInterface
        fun stopRuntime() {
            runOnUiThread { RuntimeService.stop(this@LauncherActivity) }
        }

        /** Battery settings, so the user can let OpenShell run unrestricted (always-on server). */
        @JavascriptInterface
        fun batterySettings() {
            runOnUiThread { startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)) }
        }
    }

    override fun onDestroy() {
        web.destroy()
        super.onDestroy()
    }
}
