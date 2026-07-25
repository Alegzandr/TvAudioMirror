// Genere l'icone source de l'application, en PNG, sans dependance externe.
// Le motif : un point d'emission et deux fronts d'onde qui s'en detachent.
//
//   node tools/make-icon.mjs [taille] [chemin]
//   npx tauri icon src-tauri/icons/source.png   (derive tous les formats)

import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

const SIZE = Number(process.argv[2] ?? 1024);
const OUT = process.argv[3] ?? "src-tauri/icons/source.png";

const BACKGROUND = [0x14, 0x16, 0x1a, 0xff];
const FOREGROUND = [0xec, 0xea, 0xe5, 0xff];

// Geometrie en coordonnees normalisees, pour rester nette a toutes les tailles.
const CORNER_RADIUS = 0.2;
const EMITTER = { x: 0.315, y: 0.5, r: 0.082 };
const WAVES = [
  { radius: 0.215, thickness: 0.05 },
  { radius: 0.345, thickness: 0.05 },
];
const WAVE_HALF_ANGLE = (58 * Math.PI) / 180;

// Distance signee d'un point au bord d'un carre arrondi centre en (0.5, 0.5).
function roundedSquareDistance(x, y) {
  const half = 0.5 - CORNER_RADIUS;
  const dx = Math.max(Math.abs(x - 0.5) - half, 0);
  const dy = Math.max(Math.abs(y - 0.5) - half, 0);
  return Math.hypot(dx, dy) - CORNER_RADIUS;
}

// Couverture du motif au point donne, entre 0 et 1, sans lissage :
// l'antialiasing vient du suréchantillonnage de la boucle principale.
function foregroundAt(x, y) {
  const dx = x - EMITTER.x;
  const dy = y - EMITTER.y;
  const distance = Math.hypot(dx, dy);

  if (distance <= EMITTER.r) return true;

  // Les fronts d'onde n'occupent qu'un secteur ouvert vers la droite.
  const angle = Math.atan2(dy, dx);
  if (Math.abs(angle) > WAVE_HALF_ANGLE) return false;

  for (const wave of WAVES) {
    if (Math.abs(distance - wave.radius) <= wave.thickness / 2) return true;
  }
  return false;
}

const SAMPLES = 4;
const raster = Buffer.alloc(SIZE * SIZE * 4);

for (let py = 0; py < SIZE; py++) {
  for (let px = 0; px < SIZE; px++) {
    let inside = 0;
    let covered = 0;

    for (let sy = 0; sy < SAMPLES; sy++) {
      for (let sx = 0; sx < SAMPLES; sx++) {
        const x = (px + (sx + 0.5) / SAMPLES) / SIZE;
        const y = (py + (sy + 0.5) / SAMPLES) / SIZE;
        if (roundedSquareDistance(x, y) > 0) continue;
        inside++;
        if (foregroundAt(x, y)) covered++;
      }
    }

    const total = SAMPLES * SAMPLES;
    const alpha = inside / total;
    const mix = inside === 0 ? 0 : covered / inside;
    const offset = (py * SIZE + px) * 4;

    for (let c = 0; c < 3; c++) {
      raster[offset + c] = Math.round(
        BACKGROUND[c] * (1 - mix) + FOREGROUND[c] * mix,
      );
    }
    raster[offset + 3] = Math.round(alpha * 255);
  }
}

// Encodage PNG : une passe de filtre "None" par ligne suffit, deflate fait le reste.
const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(buf) {
  let c = 0xffffffff;
  for (const byte of buf) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // profondeur
ihdr[9] = 6; // RGBA

const scanlines = Buffer.alloc(SIZE * (SIZE * 4 + 1));
for (let y = 0; y < SIZE; y++) {
  scanlines[y * (SIZE * 4 + 1)] = 0;
  raster.copy(scanlines, y * (SIZE * 4 + 1) + 1, y * SIZE * 4, (y + 1) * SIZE * 4);
}

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(scanlines, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);

mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, png);
console.log(`${OUT} ecrit (${SIZE}x${SIZE}, ${png.length} octets)`);
