// Offline catalog and fixed-text check. Run: node scripts/check-i18n.cjs
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const languages = ['en', 'es', 'pt', 'fr', 'de', 'ja'];

function files(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap(e => {
    const p = path.join(dir, e.name);
    return e.isDirectory() ? (e.name === 'audio' ? [] : files(p)) : p.endsWith('.rs') ? [p] : [];
  });
}

// Lex strings before comments, so URLs, escaped quotes, raw strings and nested
// block comments cannot confuse the check. Offsets are kept for diagnostics.
function literals(source) {
  const re = /\/\/[^\n]*|\/\*|(?:b?r)(#+)?"|b?"(?:\\[\s\S]|[^"\\])*"|'(?:\\(?:u\{[^}]+\}|x..|.)|[^'\\\r\n])'/gy;
  const out = [];
  for (let i = 0; i < source.length;) {
    re.lastIndex = i;
    const m = re.exec(source);
    if (!m) { i++; continue; }
    const token = m[0];
    if (token === '/*') {
      let depth = 1; i += 2;
      while (i < source.length && depth) {
        if (source.startsWith('/*', i)) { depth++; i += 2; }
        else if (source.startsWith('*/', i)) { depth--; i += 2; }
        else i++;
      }
      continue;
    }
    if (/^b?r#*"$/.test(token)) {
      const end = '"' + (m[1] || '');
      const start = i + token.length;
      const finish = source.indexOf(end, start);
      if (finish < 0) throw new Error('Unterminated raw string');
      out.push({ value: source.slice(start, finish), index: i, raw: true });
      i = finish + end.length; continue;
    }
    if (token.startsWith('"') || token.startsWith('b"')) {
      out.push({ value: token.slice(token.indexOf('"') + 1, -1), index: i });
    }
    i += token.length;
  }
  return out;
}

function check() {
  const errors = [];
  const catalogs = Object.fromEntries(languages.map(l => [l, JSON.parse(fs.readFileSync(path.join(root, 'src/i18n', l + '.json'), 'utf8'))]));
  const en = catalogs.en;
  // Include named slots and repeated occurrences, not just numbered slots.
  const slots = v => [...v.matchAll(/\{[^{}]+\}/g)].map(m => m[0]).sort().join(',');
  for (const [lang, catalog] of Object.entries(catalogs)) {
    for (const [key, value] of Object.entries(en)) {
      if (typeof catalog[key] !== 'string' || !catalog[key].trim()) errors.push(`${lang}: missing ${key}`);
      else if (slots(value) !== slots(catalog[key])) errors.push(`${lang}: placeholders differ for ${key}`);
    }
    for (const key of Object.keys(catalog)) if (!(key in en)) errors.push(`${lang}: extra ${key}`);
    for (const [key, value] of Object.entries(catalog)) {
      if (typeof value !== 'string') continue;
      if (/\uFFFD|\?{2,}|\p{L}\?\p{L}/u.test(value)) errors.push(`${lang}: damaged text for ${key}`);
    }
  }
  const allowed = JSON.parse(fs.readFileSync(path.join(__dirname, 'i18n-technical.json'), 'utf8'));
  for (const file of files(path.join(root, 'src'))) {
    const relative = path.relative(root, file).replaceAll('\\', '/');
    if (relative.startsWith('src/i18n/')) continue;
    const source = fs.readFileSync(file, 'utf8').split('#[cfg(test)]')[0];
    for (const { value, index } of literals(source)) {
      const before = source.slice(Math.max(0, index - 160), index);
      const line = source.slice(0, index).split('\n').length;
      if (/i18n::(?:t|text)\(\s*$/.test(before)) {
        if (!(value in en)) errors.push(`${relative}:${line}: unknown key ${value}`);
        continue;
      }
      if (!relative.startsWith('src/ui/')) continue; // backend diagnostics and protocol values
      if (/^\s*$/.test(value) || !/[\p{L}]/u.test(value.replace(/\{[^}]*\}/g, ''))) continue;
      if (allowed[relative]?.includes(value)) continue;
      // Stable IDs, shortcuts, paths and protocol parameters are not prose.
      // Visible sinks are checked even when the literal happens to look like an ID.
      const sink = /\.(?:child|set_placeholder|notify_toast)\(\s*$/.test(before) || /TextInput::new\(\s*$/.test(before);
      if (!sink && (/^[a-z0-9_./:@+\-=]+$/.test(value) || /^(?:ABYSS|TIDE)_[A-Z_]+$/.test(value))) continue;
      if (!sink && /(?:log::\w+!|include_bytes!|env!)\(\s*$/.test(before)) continue;
      if (!sink && /format!\(\s*$/.test(before) && /^[a-z0-9_.:/@+\-=\s]*$/.test(value.replace(/\{[^}]*\}/g, ''))) continue;
      if (!sink && (/^https?:\/\//.test(value) || /^<(?:path|rect|circle|svg)\b/.test(value))) continue;
      errors.push(`${relative}:${line}: fixed text ${JSON.stringify(value)}`);
    }
  }
  if (errors.length) { console.error(errors.join('\n')); process.exitCode = 1; }
  else console.log(`i18n: ${Object.keys(en).length} keys, six complete catalogs, no unreviewed fixed UI text.`);
}

module.exports = { literals };
if (require.main === module) check();
