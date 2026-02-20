#!/usr/bin/env node
/**
 * Generate AccessPilot extension icons (blue compass/assist style)
 */
const fs = require("fs");
const path = require("path");
const { PNG } = require("pngjs");

const sizes = [16, 48, 128];
const outDir = path.join(__dirname, "..", "extension", "public", "assets", "icons");

if (!fs.existsSync(outDir)) fs.mkdirSync(outDir, { recursive: true });

const bg = { r: 37, g: 99, b: 235, a: 255 }; // blue-600
const fg = { r: 255, g: 255, b: 255, a: 255 };

for (const size of sizes) {
  const png = new PNG({ width: size, height: size });
  const pad = Math.max(2, Math.floor(size * 0.15));
  const cx = size / 2;
  const cy = size / 2;

  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const idx = (size * y + x) << 2;
      const dx = (x - cx) / (size / 2);
      const dy = (y - cy) / (size / 2);
      const dist = Math.sqrt(dx * dx + dy * dy);

      if (dist <= 1) {
        png.data[idx] = bg.r;
        png.data[idx + 1] = bg.g;
        png.data[idx + 2] = bg.b;
        png.data[idx + 3] = bg.a;

        const angle = Math.atan2(dy, dx);
        const inArrow = dist < 0.7 && (angle < 0.2 || angle > Math.PI - 0.2);
        const isCenter = dist < 0.2;
        if (inArrow || isCenter) {
          png.data[idx] = fg.r;
          png.data[idx + 1] = fg.g;
          png.data[idx + 2] = fg.b;
        }
      } else {
        png.data[idx] = 0;
        png.data[idx + 1] = 0;
        png.data[idx + 2] = 0;
        png.data[idx + 3] = 0;
      }
    }
  }

  fs.writeFileSync(path.join(outDir, `icon${size}.png`), PNG.sync.write(png));
  console.log(`Wrote icon${size}.png`);
}
