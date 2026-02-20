/**
 * AccessPilot Desktop - Node server fallback
 * Serves the UI and opens in default browser. Use when Electron has module resolution issues.
 *
 * Run: node server.js
 * Then press Ctrl+Shift+Space to focus your browser (bookmark the URL).
 */

const http = require("http");
const fs = require("fs");
const path = require("path");
const { exec } = require("child_process");

const PORT = 3847;
const RENDERER = path.join(__dirname, "renderer");

const MIME = {
  ".html": "text/html",
  ".js": "application/javascript",
  ".css": "text/css",
};

const server = http.createServer((req, res) => {
  // API: open URL in default browser
  if (req.url.startsWith("/open") && req.method === "GET") {
    const idx = req.url.indexOf("url=");
    const url = idx >= 0 ? decodeURIComponent(req.url.slice(idx + 4).replace(/&.*/, "")) : "";
    if (url) exec(`open "${url.replace(/"/g, '\\"')}"`, () => {});
    res.writeHead(200, { "Access-Control-Allow-Origin": "*" });
    res.end("ok");
    return;
  }

  let url = req.url === "/" ? "/index.html" : req.url;
  url = url.split("?")[0];
  const file = path.join(RENDERER, url);

  if (!file.startsWith(RENDERER)) {
    res.writeHead(403);
    res.end();
    return;
  }

  fs.readFile(file, (err, data) => {
    if (err) {
      res.writeHead(404);
      res.end("Not found");
      return;
    }
    const ext = path.extname(file);
    res.setHeader("Content-Type", MIME[ext] || "application/octet-stream");
    res.end(data);
  });
});

server.listen(PORT, () => {
  const url = `http://localhost:${PORT}`;
  console.log(`AccessPilot running at ${url}`);
  exec(`open "${url}"`);
  console.log("Press Ctrl+Shift+Space to focus the browser. Bookmark the URL for quick access.");
});
