if (typeof window === "undefined") {
    self.addEventListener("install", () => self.skipWaiting());
    self.addEventListener("activate", (e) => e.waitUntil(self.clients.claim()));

    self.addEventListener("fetch", (e) => {
        if (e.request.mode === "navigate") {
            e.respondWith(
                fetch(e.request).then((r) => {
                    const h = new Headers(r.headers);
                    h.set("Cross-Origin-Embedder-Policy", "credentialless");
                    h.set("Cross-Origin-Opener-Policy", "same-origin");
                    return new Response(r.body, {
                        status: r.status,
                        statusText: r.statusText,
                        headers: h,
                    });
                })
            );
        }
    });
} else {
    (async () => {
        if (window.crossOriginIsolated || !("serviceWorker" in navigator)) return;

        const reg = await navigator.serviceWorker.register("coi-serviceworker.js");

        if (reg.active && !navigator.serviceWorker.controller) {
            window.location.reload();
        } else {
            const sw = reg.installing || reg.waiting;
            if (sw) {
                sw.addEventListener("statechange", () => {
                    if (sw.state === "activated") window.location.reload();
                });
            }
        }
    })();
}

