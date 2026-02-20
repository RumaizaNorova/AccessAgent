/**
 * Content script entry - injects agent + palette, handles messages
 */

import { runCommand, setPanic, onStep, removeHighlight } from "./agent";

const PALETTE_ID = "accesspilot-command-palette";
let paletteRoot: ShadowRoot | null = null;

function ensurePalette(): ShadowRoot {
  let host = document.getElementById(PALETTE_ID);
  if (!host) {
    host = document.createElement("div");
    host.id = PALETTE_ID;
    document.body.appendChild(host);
    const shadow = host.attachShadow({ mode: "closed" });
    paletteRoot = shadow;

    shadow.innerHTML = `
      <style>
        :host, * { box-sizing: border-box; }
        .backdrop {
          position: fixed;
          inset: 0;
          background: rgba(0,0,0,0.4);
          z-index: 2147483645;
          display: none;
          align-items: flex-start;
          justify-content: center;
          padding-top: 10vh;
        }
        .backdrop.visible { display: flex; }
        .panel {
          width: min(560px, 92vw);
          background: #0f172a;
          border-radius: 12px;
          box-shadow: 0 25px 50px -12px rgba(0,0,0,0.5);
          overflow: hidden;
          font-family: system-ui, -apple-system, sans-serif;
        }
        .input-row {
          display: flex;
          align-items: center;
          padding: 16px;
          border-bottom: 1px solid rgba(255,255,255,0.1);
        }
        .input-row input {
          flex: 1;
          padding: 12px 16px;
          font-size: 16px;
          border: 1px solid rgba(255,255,255,0.2);
          border-radius: 8px;
          background: #1e293b;
          color: #f8fafc;
          outline: none;
        }
        .input-row input:focus {
          border-color: #3b82f6;
          box-shadow: 0 0 0 2px rgba(59,130,246,0.3);
        }
        .input-row input::placeholder { color: #64748b; }
        .suggestions {
          padding: 8px 0;
          max-height: 200px;
          overflow-y: auto;
        }
        .suggestion {
          padding: 10px 20px;
          color: #94a3b8;
          font-size: 14px;
          cursor: pointer;
          display: block;
          width: 100%;
          text-align: left;
          border: none;
          background: none;
          font-family: inherit;
        }
        .suggestion:hover, .suggestion:focus {
          background: rgba(255,255,255,0.08);
          color: #f8fafc;
          outline: none;
        }
        .log {
          padding: 16px;
          background: #1e293b;
          border-top: 1px solid rgba(255,255,255,0.1);
          max-height: 180px;
          overflow-y: auto;
          font-size: 13px;
          color: #94a3b8;
          line-height: 1.5;
        }
        .log-entry {
          display: flex;
          gap: 8px;
          margin-bottom: 6px;
          align-items: flex-start;
        }
        .log-step { color: #64748b; flex-shrink: 0; }
        .log-action { color: #f8fafc; }
        .log-success { color: #22c55e; }
        .log-fail { color: #ef4444; }
        .panic-btn {
          margin-left: 8px;
          padding: 8px 16px;
          background: #dc2626;
          color: white;
          border: none;
          border-radius: 6px;
          font-size: 13px;
          cursor: pointer;
          font-weight: 500;
        }
        .panic-btn:hover { background: #b91c1c; }
      </style>
      <div class="backdrop" role="dialog" aria-label="AccessPilot command palette">
        <div class="panel">
          <div class="input-row">
            <input type="text" placeholder="Type a command: open, search, find, click..." 
                   spellcheck="false" autocomplete="off" id="cmd-input" />
            <button type="button" class="panic-btn" id="panic-btn">Stop</button>
          </div>
          <div class="suggestions" id="suggestions"></div>
          <div class="log" id="log"></div>
        </div>
      </div>
    `;

    const backdrop = shadow.querySelector(".backdrop")!;
    const input = shadow.querySelector("#cmd-input") as HTMLInputElement;
    const suggestionsEl = shadow.querySelector("#suggestions")!;
    const logEl = shadow.querySelector("#log")!;
    const panicBtn = shadow.querySelector("#panic-btn")!;

    const COMMAND_SUGGESTIONS = [
      "open ualberta.ca",
      "open github.com",
      "search for scholarships",
      "find requirements",
      "click continue",
      "click next",
    ];

    function showSuggestions(filter: string) {
      const filtered = filter
        ? COMMAND_SUGGESTIONS.filter((s) => s.toLowerCase().includes(filter.toLowerCase()))
        : COMMAND_SUGGESTIONS.slice(0, 4);
      suggestionsEl.innerHTML = filtered
        .map(
          (s) =>
            `<button type="button" class="suggestion" data-cmd="${s.replace(/"/g, "&quot;")}">${s}</button>`
        )
        .join("");
      suggestionsEl.querySelectorAll(".suggestion").forEach((btn) => {
        btn.addEventListener("click", () => {
          input.value = (btn as HTMLButtonElement).dataset.cmd || "";
          input.dispatchEvent(new Event("input"));
          runFromInput();
        });
      });
    }

    function addLogEntry(step: number, total: number, action: string, success: boolean) {
      const entry = document.createElement("div");
      entry.className = "log-entry";
      entry.innerHTML = `
        <span class="log-step">Step ${step}/${total || 1}</span>
        <span class="log-action ${success ? "log-success" : "log-fail"}">${action}</span>
      `;
      logEl.appendChild(entry);
      logEl.scrollTop = logEl.scrollHeight;
    }

    function clearLog() {
      logEl.innerHTML = "";
    }

    async function runFromInput() {
      const cmd = input.value.trim();
      if (!cmd) return;
      clearLog();
      onStep((s) => addLogEntry(s.step, s.total, s.action + (s.reason ? ` — ${s.reason}` : ""), s.success));
      try {
        const result = await runCommand(cmd);
        addLogEntry(1, 1, result.message, result.success);
      } catch (e) {
        addLogEntry(1, 1, `Error: ${e}`, false);
      }
    }

    input.addEventListener("input", () => showSuggestions(input.value));
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") runFromInput();
      if (e.key === "Escape") {
        setPanic();
        hidePalette();
      }
    });

    panicBtn.addEventListener("click", () => {
      setPanic();
      hidePalette();
    });

    backdrop.addEventListener("click", (e) => {
      if (e.target === backdrop) hidePalette();
    });

    showSuggestions("");
  }

  return paletteRoot!;
}

function showPalette() {
  const shadow = ensurePalette();
  const backdrop = shadow.querySelector(".backdrop")!;
  const input = shadow.querySelector("#cmd-input") as HTMLInputElement;
  backdrop.classList.add("visible");
  input.value = "";
  input.focus();
  removeHighlight();
}

function hidePalette() {
  const host = document.getElementById(PALETTE_ID);
  if (host?.shadowRoot) {
    const backdrop = host.shadowRoot.querySelector(".backdrop")!;
    backdrop.classList.remove("visible");
  }
}

chrome.runtime.onMessage.addListener(
  (msg: { type: string }, _sender, sendResponse) => {
    if (msg.type === "OPEN_PALETTE") {
      showPalette();
      sendResponse({ ok: true });
    }
    if (msg.type === "PANIC") {
      setPanic();
      hidePalette();
      removeHighlight();
      sendResponse({ ok: true });
    }
  }
);
