/**
 * Service worker - command routing, storage
 */

chrome.runtime.onInstalled.addListener(() => {
  chrome.storage.local.set({
    accessMode: false,
    confirmDestructive: true,
    panicKey: "Escape",
  });
});

chrome.commands.onCommand.addListener((command) => {
  if (command === "open-command-palette") {
    chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
      const tab = tabs[0];
      if (tab?.id && tab.url && !tab.url.startsWith("chrome:")) {
        chrome.tabs.sendMessage(tab.id, { type: "OPEN_PALETTE" }).catch(() => {
          // Content script not loaded (e.g. extension page, new tab)
        });
      }
    });
  }
});
