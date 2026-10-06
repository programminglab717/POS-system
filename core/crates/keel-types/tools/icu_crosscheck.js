#!/usr/bin/env node
// Compares how Keel shows amounts and quantities with how ICU shows them, through Node's
// Intl.NumberFormat. Keel follows CLDR's data as ICU applies it, so the two must agree, with ICU
// told to show what Keel always does: each currency's ISO 4217 decimals, and up to six decimals of
// a quantity, the millionths Keel holds quantities in. Node must carry ICU with the CLDR release
// of data/cldr/: Node 22.22 has ICU 78.2, with CLDR 48.0. See data/cldr/README.md.
//
//   cargo run -q -p keel-types --example locale_dump > /tmp/keel-locale.tsv
//   node core/crates/keel-types/tools/icu_crosscheck.js /tmp/keel-locale.tsv

'use strict';

const fs = require('fs');

// The CLDR release of data/cldr/: ICU with another one may differ from Keel by design.
const CLDR = '48';
if (process.versions.cldr === undefined || process.versions.cldr.split('.')[0] !== CLDR) {
  console.error(`Node's ICU has CLDR ${process.versions.cldr}, not ${CLDR}, the snapshot's release`);
  process.exit(2);
}

const [path] = process.argv.slice(2);
if (!path) {
  console.error('usage: icu_crosscheck.js <the output of the locale_dump example>');
  process.exit(2);
}

// `minor` units with `decimals` decimal places, as a decimal string Intl.NumberFormat takes
// exactly, without going through a floating-point number.
function decimal(minor, decimals) {
  const negative = minor.startsWith('-');
  const digits = (negative ? minor.slice(1) : minor).padStart(decimals + 1, '0');
  const integer = digits.slice(0, digits.length - decimals);
  const fraction = digits.slice(digits.length - decimals);
  return (negative ? '-' : '') + integer + (decimals > 0 ? '.' + fraction : '');
}

const escape = (text) =>
  JSON.stringify(text).replace(/[\u0080-￿]/g, (c) => '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0'));

const formats = new Map();
let total = 0;
let differences = 0;
for (const line of fs.readFileSync(path, 'utf8').split('\n').filter(Boolean)) {
  const [kind, locale, code, places, value, keel] = line.split('\t');
  const decimals = Number(places);
  const key = [kind, locale, code, places].join(' ');
  if (!formats.has(key)) {
    formats.set(
      key,
      kind === 'M'
        ? new Intl.NumberFormat(locale, {
            style: 'currency',
            currency: code,
            minimumFractionDigits: decimals,
            maximumFractionDigits: decimals,
          })
        : new Intl.NumberFormat(locale, { minimumFractionDigits: 0, maximumFractionDigits: 6 }),
    );
  }
  const icu = formats.get(key).format(decimal(value, decimals));
  total += 1;
  if (icu !== keel) {
    differences += 1;
    if (differences <= 40) {
      console.log(`${kind} ${locale} ${code} ${value}: keel ${escape(keel)}, icu ${escape(icu)}`);
    }
  }
}
console.log(
  `ICU ${process.versions.icu} (CLDR ${process.versions.cldr}): ${total} compared, ${differences} different`,
);
process.exit(differences === 0 && total > 0 ? 0 : 1);
