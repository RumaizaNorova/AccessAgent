const { app, BrowserWindow, globalShortcut, shell, ipcMain } = require("electron");
const path = require("path");

let mainWindow = null;

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 420,
    height: 320,
    minWidth: 360,
    minHeight: 280,
    frame: true,
    alwaysOnTop: true,
    resizable: true,
    skipTaskbar: true,
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  mainWindow.loadFile(path.join(__dirname, "renderer", "index.html"));

  mainWindow.on("closed", () => {
    mainWindow = null;
  });
}

app.whenReady().then(() => {
  ipcMain.handle("open-external", (_event, url) => {
    shell.openExternal(url);
  });

  createWindow();

  globalShortcut.register("CommandOrControl+Shift+Space", () => {
    if (mainWindow) {
      mainWindow.show();
      mainWindow.focus();
      mainWindow.webContents.send("agent-summon");
    }
  });
});

app.on("window-all-closed", () => {
  globalShortcut.unregisterAll();
  app.quit();
});

app.on("activate", () => {
  if (mainWindow) mainWindow.show();
});
