const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("electronAPI", {
  openExternal: (url) => ipcRenderer.invoke("open-external", url),
  onSummon: (cb) => ipcRenderer.on("agent-summon", cb),
});
ipcRenderer.setMaxListeners(1);
