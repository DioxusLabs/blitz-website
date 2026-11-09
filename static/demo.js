// Click-to-load for the wasm demo page: the apps are multi-megabyte
// downloads, so nothing is fetched until a button is pressed.
(function () {
  const frame = document.getElementById("demo-frame");
  const status = document.getElementById("demo-status");
  const links = document.getElementById("demo-links");
  const openLink = document.getElementById("demo-open-link");
  const buttons = document.querySelectorAll("[data-demo-src]");

  function hasWebGl2() {
    try {
      return !!document.createElement("canvas").getContext("webgl2");
    } catch (e) {
      return false;
    }
  }

  let poll = null;

  function load(button) {
    const src = button.dataset.demoSrc;
    buttons.forEach((b) => b.classList.toggle("demo-button--active", b === button));
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
    iframe.title = button.textContent;
    iframe.className = "demo-iframe demo-iframe--loading";
    frame.appendChild(iframe);

    // Trunk's loader sets `window.wasmBindings` once the wasm has started
    poll = setInterval(() => {
      let started = false;
      try {
        started = !!iframe.contentWindow.wasmBindings;
      } catch (e) {}
      if (started) {
        clearInterval(poll);
        status.hidden = true;
        iframe.classList.remove("demo-iframe--loading");
        iframe.focus();
      }
    }, 100);
  }

  buttons.forEach((button) => button.addEventListener("click", () => load(button)));
})();
