const confirmEl = document.getElementById("confirm-destructive") as HTMLInputElement;
const accessModeEl = document.getElementById("access-mode") as HTMLInputElement;

chrome.storage.local.get(["confirmDestructive", "accessMode"], (data) => {
  confirmEl.checked = data.confirmDestructive ?? true;
  accessModeEl.checked = !!data.accessMode;
});

confirmEl.addEventListener("change", () => {
  chrome.storage.local.set({ confirmDestructive: confirmEl.checked });
});
accessModeEl.addEventListener("change", () => {
  chrome.storage.local.set({ accessMode: accessModeEl.checked });
});
