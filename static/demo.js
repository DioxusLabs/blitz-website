// The demo page's browser UI around the Blitz renderer. The renderer is a
// multi-megabyte download, so nothing is fetched until it's asked for.
//
// Blitz view: the renderer's index.html in an iframe, loaded with ?proxy= and
// ?url=. It posts {type: "blitz:ready"}, {type: "blitz:navigated", url, title}
// and {type: "blitz:error", url, message}, and takes {type: "blitz:navigate", url}.
//
// Native view: /proxy/frame?url=, the same page rendered by this browser. It
// posts {type: "frame:loaded", url, title} and {type: "frame:navigate", url}.
(function () {
  const form = document.getElementById("demo-form");
  const urlInput = document.getElementById("demo-url");
  const backButton = document.getElementById("demo-back");
  const forwardButton = document.getElementById("demo-forward");
  const reloadButton = document.getElementById("demo-reload");
  const viewButtons = document.querySelectorAll(".demo-view");
  const statusLine = document.getElementById("demo-status-line");
  const frame = document.getElementById("demo-frame");
  const overlay = document.getElementById("demo-overlay");
  const overlayText = document.getElementById("demo-overlay-text");
  const loadButton = document.getElementById("demo-load");

  // The renderer fetches pages and subresources as this prefix + the URL-encoded target
  const PROXY_PREFIX = "/proxy?url=";

  // The pages visited, shared by both views
  const history = [];
  let historyIndex = -1;
  let activeView = "blitz";
  let blitzAllowed = false;

  // Per-view state: its iframe, the URL it shows, and the title it reported
  const views = {
    blitz: { iframe: null, url: null, title: null, error: null, ready: false, started: false },
    native: { iframe: null, url: null, title: null, error: null },
  };

  function currentUrl() {
    return history[historyIndex] || null;
  }

  function normalizeUrl(value) {
    value = value.trim();
    if (!value) return null;
    if (!/^https?:\/\//i.test(value)) value = "https://" + value;
    try {
      return new URL(value).href;
    } catch (e) {
      return null;
    }
  }

  function hasWebGl2() {
    try {
      return !!document.createElement("canvas").getContext("webgl2");
    } catch (e) {
      return false;
    }
  }

  function describeError(message) {
    if (/\b429\b|too many/i.test(message)) {
      return "Too many requests through the demo's proxy; try again in a minute.";
    }
    return message;
  }

  function updateUi() {
    backButton.disabled = historyIndex <= 0;
    forwardButton.disabled = historyIndex >= history.length - 1;
    reloadButton.disabled = !currentUrl();

    for (const button of viewButtons) {
      button.setAttribute("aria-pressed", String(button.dataset.view === activeView));
    }
    for (const [name, view] of Object.entries(views)) {
      if (view.iframe) view.iframe.hidden = name !== activeView;
    }

    const view = views[activeView];
    const showOverlay = activeView === "blitz" && !blitzAllowed;
    overlay.hidden = !showOverlay;

    statusLine.classList.toggle("demo-status-line--error", !!view.error);
    if (showOverlay) {
      statusLine.textContent = "";
    } else if (view.error) {
      statusLine.textContent = view.error;
    } else if (activeView === "blitz" && !view.started) {
      statusLine.textContent = "Loading Blitz…";
    } else if (view.url !== currentUrl() || view.title === null) {
      statusLine.textContent = "Loading…";
    } else {
      statusLine.textContent = view.title || view.url;
    }
  }

  function createIframe(name, src, title, sandbox) {
    const iframe = document.createElement("iframe");
    iframe.className = "demo-iframe";
    iframe.title = title;
    if (sandbox !== undefined) iframe.setAttribute("sandbox", sandbox);
    iframe.src = src;
    frame.appendChild(iframe);
    views[name].iframe = iframe;
    return iframe;
  }

  function showInBlitz(url) {
    const view = views.blitz;
    if (!blitzAllowed || view.url === url) return;
    view.url = url;
    view.title = null;
    view.error = null;

    if (!hasWebGl2()) {
      view.error = "Blitz needs WebGL2, which isn't available in this browser.";
      return;
    }
    if (view.iframe && view.ready) {
      view.iframe.contentWindow.postMessage({ type: "blitz:navigate", url }, location.origin);
      return;
    }

    // Renderers that don't take messages are reloaded with the new ?url=
    view.iframe?.remove();
    view.ready = false;
    view.started = false;
    const src =
      form.dataset.demoSrc +
      "?proxy=" + encodeURIComponent(PROXY_PREFIX) +
      "&url=" + encodeURIComponent(url);
    const iframe = createIframe("blitz", src, "Page rendered by Blitz");
    // Fallback start detection for renderers without blitz:ready: the
    // dioxus-cli loader sets `__dx_mainWasm` once the wasm has started
    const poll = setInterval(() => {
      if (views.blitz.iframe !== iframe) return clearInterval(poll);
      let started = false;
      try {
        started = !!iframe.contentWindow.__dx_mainWasm;
      } catch (e) {}
      if (started) {
        clearInterval(poll);
        markBlitzStarted();
      }
    }, 100);
  }

  function markBlitzStarted() {
    const view = views.blitz;
    if (view.started) return;
    view.started = true;
    // Without blitz:navigated the title is unknown, so show the URL
    if (!view.ready && view.title === null) view.title = "";
    updateUi();
  }

  function showInNative(url) {
    const view = views.native;
    if (view.url === url) return;
    view.url = url;
    view.title = null;
    view.error = null;
    const src = "/proxy/frame?url=" + encodeURIComponent(url);
    if (view.iframe) {
      view.iframe.src = src;
    } else {
      // No allow-same-origin: the page can't reach this site's origin
      createIframe("native", src, "Page rendered by your browser", "allow-scripts");
    }
  }

  // Show the current page in the active view; the other view catches up
  // when it's switched to
  function show() {
    const url = currentUrl();
    if (!url) return;
    urlInput.value = url;
    if (activeView === "blitz") showInBlitz(url);
    else showInNative(url);
    updateUi();
  }

  function navigate(url) {
    if (!url || url === currentUrl()) return show();
    history.splice(historyIndex + 1);
    history.push(url);
    historyIndex = history.length - 1;
    show();
  }

  // A view reported the page it's showing: the page it was asked for (maybe
  // after redirects) while it has no title yet, or one it navigated to itself
  function navigatedWithin(name, url) {
    if (views[name].title === null && views[name].url === currentUrl()) {
      history[historyIndex] = url;
      urlInput.value = url;
    } else if (url !== currentUrl()) {
      history.splice(historyIndex + 1);
      history.push(url);
      historyIndex = history.length - 1;
      urlInput.value = url;
    }
    views[name].url = url;
  }

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const url = normalizeUrl(urlInput.value);
    if (!url) {
      statusLine.textContent = "That isn't a valid URL.";
      statusLine.classList.add("demo-status-line--error");
      return;
    }
    // Go is asking for the page, so it's also asking for the renderer
    if (activeView === "blitz") blitzAllowed = true;
    navigate(url);
  });

  loadButton.addEventListener("click", () => {
    blitzAllowed = true;
    navigate(normalizeUrl(urlInput.value));
  });

  backButton.addEventListener("click", () => {
    if (historyIndex > 0) {
      historyIndex--;
      show();
    }
  });
  forwardButton.addEventListener("click", () => {
    if (historyIndex < history.length - 1) {
      historyIndex++;
      show();
    }
  });
  reloadButton.addEventListener("click", () => {
    views[activeView].url = null;
    show();
  });

  for (const button of viewButtons) {
    button.addEventListener("click", () => {
      activeView = button.dataset.view;
      if (currentUrl()) show();
      else if (activeView === "native") navigate(normalizeUrl(urlInput.value));
      else updateUi();
    });
  }

  window.addEventListener("message", (event) => {
    const data = event.data;
    if (!data || typeof data.type !== "string") return;

    const blitz = views.blitz;
    if (blitz.iframe && event.source === blitz.iframe.contentWindow) {
      if (event.origin !== location.origin) return;
      if (data.type === "blitz:ready") {
        blitz.ready = true;
        markBlitzStarted();
      } else if (data.type === "blitz:navigated") {
        markBlitzStarted();
        navigatedWithin("blitz", String(data.url));
        blitz.title = String(data.title || "");
        blitz.error = null;
      } else if (data.type === "blitz:error") {
        markBlitzStarted();
        if (data.url) navigatedWithin("blitz", String(data.url));
        blitz.title = "";
        blitz.error = "Couldn't load " + (data.url || "the page") + ": " +
          describeError(String(data.message || "unknown error"));
      }
      updateUi();
      return;
    }

    // The native view has an opaque origin, so it's identified by its window
    const native = views.native;
    if (native.iframe && event.source === native.iframe.contentWindow) {
      if (data.type === "frame:loaded") {
        native.title = String(data.title || "");
      } else if (data.type === "frame:navigate") {
        navigate(normalizeUrl(String(data.url)));
      }
      updateUi();
    }
  });

  // The native view's errors are plain text from the proxy, which doesn't
  // post frame:loaded; show its URL once the frame has loaded anyway
  frame.addEventListener("load", (event) => {
    const native = views.native;
    if (event.target === native.iframe && native.title === null) {
      native.title = "";
      updateUi();
    }
  }, true);

  updateUi();
})();
