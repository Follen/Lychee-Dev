import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, writeFileSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import {inspectSource, inventory} from './rust-unsafe-boundaries.mjs';

const guard = '#![forbid(unsafe_code)]\n';
test('text, nested comments, raw identifiers, byte/C/raw strings are not unsafe syntax', () => {
  const source = guard + `
    // unsafe { ignored() }
    /* unsafe fn x() {} /* unsafe impl Send for X {} */ */
    const MESSAGE: &str = "unsafe relative path";
    let raw = r###" unsafe { \" } "###;
    let bytes = br#"unsafe fn x"#;
    let c = c"unsafe impl X";
    let raw_c = cr##"unsafe extern"##;
    let quote = '\\'';
    let unicode = '\\u{1f600}';
    let r#unsafe = 1;
    fn borrowed<'a>(v: &'a str) -> &'a str { v }
  `;
  assert.deepEqual(inspectSource(source), {forbidden: true, sites: []});
});

test('distinguishes blocks, declarations, impls, unsafe attributes and assembly', () => {
  const source = `unsafe { f() }\nunsafe fn f() {}\nunsafe extern "system" { fn g(); }
unsafe extern "system" fn callback() {}\nunsafe impl Send for X {}\nunsafe trait T {}
#[unsafe(no_mangle)] static X: u8 = 0;\nstd::arch::global_asm!("ret");
#[no_mangle] static Y: u8 = 0;\nextern "C" { fn old(); }`;
  assert.deepEqual(inspectSource(source).sites.map(site => site.kind), [
    'block', 'fn', 'extern_block', 'extern_fn', 'impl', 'trait',
    'attribute', 'assembly_macro', 'legacy_unsafe_attribute', 'legacy_extern_block',
  ]);
});

test('only an unconditional crate-level forbid satisfies the guard', () => {
  for (const source of [
    '// #![forbid(unsafe_code)]', '"#![forbid(unsafe_code)]"',
    '#[forbid(unsafe_code)] mod module {}', '#![deny(unsafe_code)]',
    '#![cfg_attr(not(test), forbid(unsafe_code))]',
  ]) assert.equal(inspectSource(source).forbidden, false);
  assert.equal(inspectSource('//! docs\n' + guard).forbidden, true);
});

test('line numbers survive multiline strings and nested comments', () => {
  assert.deepEqual(inspectSource('r#"one\ntwo"#\n/* three\n/*four*/ */\nunsafe {}').sites,
    [{line: 5, kind: 'block'}]);
});

test('unterminated literals and comments fail closed', () => {
  for (const source of ['"unsafe', 'r##"unsafe"#', '/* /* nested */'])
    assert.throws(() => inspectSource(source), /unterminated/);
});

test('business unsafe, guard removal and unknown fixture unsafe are rejected', () => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-unsafe-inventory-'));
  const put = (path, text) => {
    const absolute = join(root, path);
    mkdirSync(join(absolute, '..'), {recursive: true});
    writeFileSync(absolute, text);
  };
  try {
    for (const name of ['cli', 'core', 'data', 'delivery', 'emmy-adapter', 'live', 'source'])
      put(`crates/lycheedev-${name}/src/${name === 'cli' ? 'main' : 'lib'}.rs`, guard);
    put('crates/lycheedev-windows/src/boundary.rs', 'unsafe extern "system" { fn api(); }');
    assert.deepEqual(inventory(root).violations, []);
    put('crates/lycheedev-emmy-adapter/src/lib.rs', '// adapter guard removed');
    assert.deepEqual(inventory(root).violations,
      ['missing unconditional crate forbid: crates/lycheedev-emmy-adapter/src/lib.rs']);
    put('crates/lycheedev-emmy-adapter/src/lib.rs', guard);
    put('crates/lycheedev-core/src/new.rs', 'unsafe { bad() }');
    put('crates/lycheedev-data/src/lib.rs', '// guard removed');
    put('crates/lycheedev-live/tests/new.rs', 'unsafe { bad() }');
    const report = inventory(root);
    assert.equal(report.violations.length, 3);
    assert.equal(report.files.find(file => file.path.includes('windows')).scope,
      'windows_boundary_pending_review');
    assert.match(report.dependencyAudit, /^not_run/);
  } finally { rmSync(root, {recursive: true, force: true}); }
});
