package dev.openshell.android

import android.app.Activity
import android.content.ComponentCallbacks2
import android.content.Intent
import android.os.Bundle
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import org.json.JSONObject

/**
 * Shows one running OpenShell app: its frontend in a WebView. The page has no access to Android
 * APIs (no JavaScript interface); it talks to its own services over 127.0.0.1 like on the desktop.
 * Forwards foreground / background / low-memory events to the core.
 */
class AppActivity : Activity() {
    companion object {
        const val EXTRA_IDENTIFIER = "identifier"
    }

    private lateinit var web: WebView
    private var loaded: String? = null
    private val onStatus: (JSONObject) -> Unit = { show(it) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        web = WebView(this)
        web.settings.javaScriptEnabled = true
        web.settings.domStorageEnabled = true
        web.settings.allowFileAccess = false
        web.webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                val host = request.url.host
                if (host == "127.0.0.1" || host == "localhost") return false
                // links to the internet open in the browser, not inside the app
                startActivity(Intent(Intent.ACTION_VIEW, request.url))
                return true
            }
        }
        setContentView(web)
        web.loadData(page("Starting…", ""), "text/html", "utf-8")

        val id = intent.getStringExtra(EXTRA_IDENTIFIER)
        val app = id?.let { Apps.find(this, it) }
        if (app == null) {
            web.loadData(page("App not found", "Install it again from the launcher."), "text/html", "utf-8")
            return
        }
        title = app.optString("app")
        RuntimeService.start(this, id, app.optString("dir"), app.optString("app"))
    }

    private fun show(s: JSONObject) {
        if (!s.optBoolean("ok", true)) {
            web.loadData(page("The app could not start", s.optString("error")), "text/html", "utf-8")
            return
        }
        if (!s.optBoolean("running")) return
        val url = s.optString("frontend_url", "")
        if (url.isNotEmpty() && url != "null") {
            if (loaded != url) {
                loaded = url
                web.loadUrl(url)
            }
        } else if (loaded == null) {
            loaded = ""
            web.loadData(
                page(
                    s.optString("app"),
                    "This app has no user interface. Its functions run in the background and are available to agents and paired devices."
                ),
                "text/html",
                "utf-8"
            )
        }
    }

    private fun page(title: String, text: String): String {
        val esc = { v: String -> v.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;") }
        return "<!doctype html><meta name=viewport content='width=device-width'>" +
            "<body style='font-family:sans-serif;background:#101412;color:#e8e8e8;padding:2rem'>" +
            "<h2>${esc(title)}</h2><p>${esc(text)}</p></body>"
    }

    override fun onStart() {
        super.onStart()
        RuntimeService.addListener(onStatus)
        show(RuntimeService.status)
        RuntimeService.lifecycle(JSONObject().put("kind", "foreground"))
    }

    override fun onStop() {
        RuntimeService.removeListener(onStatus)
        RuntimeService.lifecycle(JSONObject().put("kind", "background"))
        super.onStop()
    }

    @Deprecated("Deprecated in Java")
    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        @Suppress("DEPRECATION")
        if (level >= ComponentCallbacks2.TRIM_MEMORY_RUNNING_CRITICAL) {
            RuntimeService.lifecycle(JSONObject().put("kind", "low_memory"))
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        if (web.canGoBack()) web.goBack() else @Suppress("DEPRECATION") super.onBackPressed()
    }

    override fun onDestroy() {
        web.destroy()
        super.onDestroy()
    }
}
