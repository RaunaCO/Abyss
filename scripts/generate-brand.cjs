// Original Abyss sound-wave vortex. No packages or downloaded artwork.
// SVG and raster resources share the same geometry. Run with Node.js.
const fs = require('node:fs');
const path = require('node:path');
const zlib = require('node:zlib');
const assets = path.resolve(__dirname, '../assets');

const waves = Array.from({ length: 5 }, (_, arm) => ({
  shade: [18, 46, 77, 106, 139][arm],
  width: 12,
  points: Array.from({ length: 201 }, (_, i) => {
    const t = i / 200;
    const radius = 27 + 165 * t + 3 * Math.sin(t * Math.PI * 8) * t;
    const angle = arm * Math.PI * 2 / 5 + t * 4.35 - 1.7;
    return [256 + radius * Math.cos(angle), 256 + radius * Math.sin(angle)];
  }),
}));
const hex = n => n.toString(16).padStart(2, '0').repeat(3);
const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" role="img" aria-labelledby="title">
<title id="title">Abyss</title>
<defs><linearGradient id="background" x2="0.8" y2="1"><stop stop-color="#fff"/><stop offset="1" stop-color="#dedede"/></linearGradient></defs>
<rect width="512" height="512" rx="96" fill="url(#background)"/>
${waves.map(w => `<path d="${w.points.map(([x,y], i) => `${i ? 'L' : 'M'}${x.toFixed(2)} ${y.toFixed(2)}`).join(' ')}" fill="none" stroke="#${hex(w.shade)}" stroke-width="${w.width}" stroke-linecap="round" stroke-linejoin="round"/>`).join('\n')}
</svg>\n`;
fs.writeFileSync(path.join(assets, 'logo.svg'), svg);

function raster(size) {
  const scale = size / 512, pixels = new Float64Array(size * size);
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
    pixels[y * size + x] = 255 - 33 * Math.min(1, (0.8 * x + y) / (1.64 * size));
  }
  for (const wave of waves) for (let i = 1; i < wave.points.length; i++) {
    const [ax, ay] = wave.points[i - 1].map(v => v * scale);
    const [bx, by] = wave.points[i].map(v => v * scale);
    const radius = wave.width * scale / 2, dx = bx - ax, dy = by - ay, length = dx * dx + dy * dy;
    for (let y = Math.max(0, Math.floor(Math.min(ay,by) - radius - 1)); y < Math.min(size, Math.ceil(Math.max(ay,by) + radius + 1)); y++) {
      for (let x = Math.max(0, Math.floor(Math.min(ax,bx) - radius - 1)); x < Math.min(size, Math.ceil(Math.max(ax,bx) + radius + 1)); x++) {
        const t = Math.max(0, Math.min(1, ((x + 0.5 - ax) * dx + (y + 0.5 - ay) * dy) / length));
        const coverage = Math.max(0, Math.min(1, radius + 0.5 - Math.hypot(x + 0.5 - ax - t * dx, y + 0.5 - ay - t * dy)));
        const index = y * size + x;
        pixels[index] = Math.min(pixels[index], 255 + (wave.shade - 255) * coverage);
      }
    }
  }
  return pixels;
}
function crc(bytes) {
  let n = 0xffffffff;
  for (const byte of bytes) { n ^= byte; for (let k=0; k<8; k++) n = (n >>> 1) ^ ((n & 1) ? 0xedb88320 : 0); }
  return (n ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const tag = Buffer.from(type), out = Buffer.alloc(data.length + 12);
  out.writeUInt32BE(data.length); tag.copy(out, 4); data.copy(out, 8);
  out.writeUInt32BE(crc(Buffer.concat([tag, data])), out.length - 4);
  return out;
}
function png(size) {
  const factor = 4, high = size * factor, pixels = raster(high);
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y=0; y<size; y++) for (let x=0; x<size; x++) {
    let value = 0;
    for (let dy=0;dy<factor;dy++) for (let dx=0;dx<factor;dx++) value += pixels[(y*factor+dy)*high+x*factor+dx];
    const offset = y * (size * 4 + 1) + 1 + x * 4;
    raw[offset] = raw[offset+1] = raw[offset+2] = Math.round(value / (factor*factor));
    // Match the SVG's rounded square; transparent only outside its corners.
    const px = (x + 0.5) * 512 / size, py = (y + 0.5) * 512 / size;
    const cx = Math.max(96, Math.min(416, px)), cy = Math.max(96, Math.min(416, py));
    raw[offset+3] = Math.round(255 * Math.max(0, Math.min(1, (96 - Math.hypot(px-cx,py-cy)) * size / 512 + 0.5)));
  }
  const header = Buffer.alloc(13); header.writeUInt32BE(size); header.writeUInt32BE(size, 4); header[8]=8; header[9]=6;
  return Buffer.concat([Buffer.from('89504e470d0a1a0a','hex'), chunk('IHDR',header),chunk('IDAT',zlib.deflateSync(raw)),chunk('IEND',Buffer.alloc(0))]);
}
fs.writeFileSync(path.join(assets, 'logo.png'), png(512));
const sizes = [16, 24, 32, 48, 64, 128, 256], images = sizes.map(png);
const header = Buffer.alloc(6 + 16 * sizes.length); header.writeUInt16LE(1, 2); header.writeUInt16LE(sizes.length, 4);
let offset = header.length;
sizes.forEach((size, i) => { const p=6+16*i; header[p]=header[p+1]=size%256; header.writeUInt16LE(1,p+4);header.writeUInt16LE(32,p+6);header.writeUInt32LE(images[i].length,p+8);header.writeUInt32LE(offset,p+12);offset+=images[i].length; });
fs.writeFileSync(path.join(assets, 'icon.ico'), Buffer.concat([header, ...images]));
console.log('Generated Abyss SVG, 512 px PNG and seven-size ICO.');
