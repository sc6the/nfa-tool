import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
const root = new URL('../ui/', import.meta.url);
const files = { '/': ['index.html', 'text/html'], '/index.html': ['index.html', 'text/html'], '/app.js': ['app.js', 'text/javascript'], '/style.css': ['style.css', 'text/css'], '/mark.svg': ['mark.svg', 'image/svg+xml'] };
createServer(async (req, res) => {
  const file = files[new URL(req.url, 'http://localhost').pathname];
  if (!file) { res.writeHead(404); res.end(); return; }
  try {
    const data = await readFile(new URL(file[0], root));
    res.writeHead(200, { 'Content-Type': file[1], 'Cache-Control': 'no-store' });
    res.end(data);
  } catch { res.writeHead(500); res.end('Preview unavailable'); }
}).listen(4178, '127.0.0.1', () => console.log('NFA preview: http://127.0.0.1:4178/?preview=1'));
