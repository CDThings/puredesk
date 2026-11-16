// 生成应用图标 assets/icon.png（1024x1024，圆角渐变底 + 白色收纳盒图形）。
// 零依赖：手写 PNG 编码（IHDR/IDAT/IEND + zlib deflate + CRC32）。
import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const S = 1024;
const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outPath = join(root, "assets", "icon.png");

// ---------- PNG 编码 ----------
const crcTable = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();
function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = crcTable[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}
function encodePng(width, height, rgba) {
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // color type RGBA
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (width * 4 + 1)] = 0; // filter: none
    rgba.copy(raw, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4);
  }
  const idat = deflateSync(raw, { level: 9 });
  return Buffer.concat([sig, chunk("IHDR", ihdr), chunk("IDAT", idat), chunk("IEND", Buffer.alloc(0))]);
}

// ---------- 绘制 ----------
// 圆角矩形 SDF（px 距离场），返回有符号距离（<0 在内部）
function sdfRoundRect(px, py, cx, cy, hx, hy, r) {
  const qx = Math.abs(px - cx) - (hx - r);
  const qy = Math.abs(py - cy) - (hy - r);
  const ax = Math.max(qx, 0), ay = Math.max(qy, 0);
  return Math.hypot(ax, ay) + Math.min(Math.max(qx, qy), 0) - r;
}
const clamp01 = (v) => Math.min(1, Math.max(0, v));
// 覆盖率（0.5px 平滑边缘）
const cover = (sd) => clamp01(0.5 - sd);
// 颜色混合（覆盖 alpha，通道钳制到 0-255 防回绕）
const u8 = (v) => Math.min(255, Math.max(0, Math.round(v)));
function blend(dst, i, r, g, b, a) {
  const na = 1 - a;
  dst[i] = u8(r * a + dst[i] * na);
  dst[i + 1] = u8(g * a + dst[i + 1] * na);
  dst[i + 2] = u8(b * a + dst[i + 2] * na);
  dst[i + 3] = Math.max(dst[i + 3], a * 255);
}

const data = Buffer.alloc(S * S * 4);
// 渐变端色：靛蓝 -> 青
const c0 = [91, 124, 255]; // #5b7cff
const c1 = [56, 208, 236]; // #38d0ec

for (let y = 0; y < S; y++) {
  for (let x = 0; x < S; x++) {
    const i = (y * S + x) * 4;
    // 背板：圆角 224，对角渐变
    const sdBg = sdfRoundRect(x + 0.5, y + 0.5, S / 2, S / 2, S / 2 - 6, S / 2 - 6, 224);
    const aBg = cover(sdBg);
    if (aBg <= 0) continue;
    const t = (x + y) / (2 * S);
    let r = c0[0] + (c1[0] - c0[0]) * t;
    let g = c0[1] + (c1[1] - c0[1]) * t;
    let b = c0[2] + (c1[2] - c0[2]) * t;
    // 轻微内亮高光（左上）
    const hl = clamp01(1 - Math.hypot(x - S * 0.28, y - S * 0.22) / (S * 0.75)) * 0.18;
    r += 255 * hl * 0.25; g += 255 * hl * 0.25; b += 255 * hl * 0.25;
    blend(data, i, r, g, b, aBg);
  }
}
// 盒盖（横条）
const drawRR = (cx, cy, hx, hy, r, alpha) => {
  for (let y = Math.floor(cy - hy) - 2; y <= Math.ceil(cy + hy) + 2; y++) {
    for (let x = Math.floor(cx - hx) - 2; x <= Math.ceil(cx + hx) + 2; x++) {
      if (x < 0 || y < 0 || x >= S || y >= S) continue;
      const sd = sdfRoundRect(x + 0.5, y + 0.5, cx, cy, hx, hy, r);
      const a = cover(sd) * alpha;
      if (a > 0) blend(data, (y * S + x) * 4, 255, 255, 255, a);
    }
  }
};
drawRR(S / 2, 396, 236, 34, 34, 0.82);           // 盒盖
drawRR(S / 2, 600, 256, 122, 48, 0.95);          // 盒身
drawRR(S / 2, 522, 120, 26, 26, 0.55);           // 抽屉把手
// 阴影下移一点点让盒身更有层次：再画一遍盒身底边暗色
for (let y = 690; y < 748; y++) {
  for (let x = 220; x < 804; x++) {
    const sd = sdfRoundRect(x + 0.5, y + 0.5, S / 2, 600, 256, 122, 48);
    if (sd < 0 && y > 640) {
      const i = (y * S + x) * 4;
      data[i] = data[i] * 0.92; data[i + 1] = data[i + 1] * 0.92; data[i + 2] = data[i + 2] * 0.95;
    }
  }
}

mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, encodePng(S, S, data));
console.log("icon written:", outPath);
