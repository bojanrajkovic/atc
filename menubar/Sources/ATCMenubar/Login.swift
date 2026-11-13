import AppKit
import WebKit

/// Signs in to ATC through its GitHub OAuth flow in a web view and returns
/// the session cookie.
///
/// ATC sets an HttpOnly session cookie on its own origin and has no token
/// endpoint, so a web view is the only client that can finish the flow. The
/// web view keeps its GitHub session on disk, which lets later sign-ins
/// finish without showing a window.
@MainActor
final class Login: NSObject, WKNavigationDelegate, NSWindowDelegate {
    private var window: NSWindow?
    private var server: URL?
    private var continuation: CheckedContinuation<HTTPCookie?, Never>?
    private var timeout: Task<Void, Never>?

    /// Runs the flow. With `interactive` false the window stays hidden and
    /// the flow gives up after 15 seconds, which is what happens when GitHub
    /// needs a password or a consent click.
    func signIn(server: URL, interactive: Bool) async -> HTTPCookie? {
        finish(nil)
        self.server = server

        let webView = WKWebView(frame: NSRect(x: 0, y: 0, width: 520, height: 720))
        webView.navigationDelegate = self
        let window = NSWindow(
            contentRect: webView.frame, styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = "Sign in to ATC"
        window.contentView = webView
        window.isReleasedWhenClosed = false
        window.delegate = self
        window.center()
        self.window = window

        if interactive {
            NSApp.activate()
            window.makeKeyAndOrderFront(nil)
        } else {
            timeout = Task { [weak self] in
                try? await Task.sleep(for: .seconds(15))
                if !Task.isCancelled { self?.finish(nil) }
            }
        }

        webView.load(URLRequest(url: server.appending(path: "v1/auth/github/login")))
        return await withTaskCancellationHandler {
            await withCheckedContinuation { continuation = $0 }
        } onCancel: {
            Task { @MainActor in self.finish(nil) }
        }
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        // ATC's callback redirects to a page outside /v1/auth once the session cookie is set.
        guard let server, let url = webView.url, url.host() == server.host(), !url.path().hasPrefix("/v1/auth")
        else { return }
        Task {
            let cookies = await webView.configuration.websiteDataStore.httpCookieStore.allCookies()
            // "__Host-atc_session" over https, "atc_session" over http.
            finish(cookies.first { $0.name.hasSuffix("atc_session") && $0.domain == server.host() })
        }
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        if window?.isVisible != true { finish(nil) }
    }

    func windowWillClose(_ notification: Notification) {
        finish(nil)
    }

    private func finish(_ cookie: HTTPCookie?) {
        timeout?.cancel()
        timeout = nil
        continuation?.resume(returning: cookie)
        continuation = nil
        let closing = window
        window = nil
        closing?.close()
    }
}
