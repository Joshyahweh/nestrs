(function () {
  const root = document.documentElement;
  const button = document.getElementById("theme-toggle");
  const label = document.getElementById("theme-label");
  const STORAGE_KEY = "nestrs-theme";

  function preferredTheme() {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "light" || stored === "dark") return stored;
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }

  function setTheme(theme) {
    root.setAttribute("data-theme", theme);
    label.textContent = theme === "dark" ? "Light" : "Dark";
    localStorage.setItem(STORAGE_KEY, theme);
  }

  if (button && label) {
    setTheme(preferredTheme());
    button.addEventListener("click", () => {
      const current = root.getAttribute("data-theme") || "light";
      setTheme(current === "dark" ? "light" : "dark");
    });
  }

  const docsSearch = document.getElementById("docs-search");
  if (docsSearch) {
    const cards = Array.from(document.querySelectorAll(".searchable"));
    docsSearch.addEventListener("input", (event) => {
      const value = String(event.target.value || "").toLowerCase().trim();
      cards.forEach((card) => {
        const text = String(card.getAttribute("data-search") || "").toLowerCase();
        const shouldShow = !value || text.includes(value);
        card.classList.toggle("is-hidden", !shouldShow);
      });
    });
  }
})();
