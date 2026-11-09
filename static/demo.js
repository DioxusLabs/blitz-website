// Click-to-load for the wasm demo page: the browser is a multi-megabyte
// download, so nothing is fetched until the form is submitted.
(function () {
  const form = document.getElementById("demo-form");
  const urlInput = document.getElementById("demo-url");
  const frame = document.getElementById("demo-frame");
  const status = document.getElementById("demo-status");
  const links = document.getElementById("demo-links");
  const openLink = document.getElementById("demo-open-link");

  // The app fetches pages and subresources as this prefix + the URL-encoded target
  const PROXY_PREFIX = "/proxy?url=";

  function hasWebGl2() {
    try {
      return !!document.createElement("canvas").getContext("webgl2");
    } catch (e) {
      return false;
    }
  }

  let poll = null;

  function load() {
    let startPage = urlInput.value.trim();
    if (!/^https?:\/\//i.test(startPage)) {
      startPage = "https://" + startPage;
    }
    const src =
      form.dataset.demoSrc +
      "?proxy=" + encodeURIComponent(PROXY_PREFIX) +
      "&url=" + encodeURIComponent(startPage);

    clearInterval(poll);
    frame.querySelector("iframe")?.remove();

    if (!hasWebGl2()) {
      status.textContent =
        "This demo needs WebGL2, which isn't available in this browser.";
      status.hidden = false;
      return;
    }

    status.textContent = "Loading…";
    status.hidden = false;
    links.hidden = false;
    openLink.href = src;

    const iframe = document.createElement("iframe");
    iframe.src = src;
    iframe.title = "Blitz browser";
    iframe.className = "demo-iframe demo-iframe--loading";
    frame.appendChild(iframe);

    // The dioxus-cli loader sets `__dx_mainWasm` once the wasm has started
    poll = setInterval(() => {
      let started = false;
      try {
        started = !!iframe.contentWindow.__dx_mainWasm;
      } catch (e) {}
      if (started) {
        clearInterval(poll);
        status.hidden = true;
        iframe.classList.remove("demo-iframe--loading");
        iframe.focus();
      }
    }, 100);
  }

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    load();
  });
})();
