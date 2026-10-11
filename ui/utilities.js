(() => {
  "use strict";

  const invoke = window.__TAURI__?.core?.invoke;

  document.querySelectorAll("[data-utility-url]").forEach((card) => {
    card.addEventListener("click", async () => {
      const url = card.dataset.utilityUrl;
      if (!url || card.disabled) return;
      card.disabled = true;
      card.classList.add("opening");
      try {
        if (invoke) {
          await invoke("open_external", { url });
        } else {
          window.open(url, "_blank", "noopener,noreferrer");
        }
      } catch (error) {
        if (typeof window.cgsShowModal === "function") {
          await window.cgsShowModal(
            "Не удалось открыть страницу",
            String(error),
            { symbol: "!" },
          );
        }
      } finally {
        card.disabled = false;
        card.classList.remove("opening");
      }
    });
  });
})();
