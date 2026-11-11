// Extension metadata checks. Run with:
//   gjs -m gnome-extension/tests/metadata.test.js
//
// The shell-version list is the contract that makes GNOME Shell load the
// extension. Keep it in sync with the ESM API revisions the extension supports.

import GLib from 'gi://GLib';

const EXPECTED_SHELL_VERSIONS = [
    '45', '46', '47', '48', '49', '50', '51',
];
const MINIMUM_EXTENSION_VERSION = 4;

let failures = 0;

function fail(message) {
    failures += 1;
    printerr(`FAIL: ${message}`);
}

function equal(actual, expected, label) {
    const left = JSON.stringify(actual);
    const right = JSON.stringify(expected);
    if (left !== right)
        fail(`${label}: expected ${right}, got ${left}`);
}

function check(condition, label) {
    if (!condition)
        fail(label);
}

const [testPath] = GLib.filename_from_uri(import.meta.url);
const metadataPath = GLib.build_filenamev([
    GLib.path_get_dirname(testPath),
    '..',
    'dimmer@dimmer.linux',
    'metadata.json',
]);

const [read, contents] = GLib.file_get_contents(metadataPath);
check(read, `could not read ${metadataPath}`);

let metadata = null;
if (read) {
    try {
        metadata = JSON.parse(new TextDecoder().decode(contents));
    } catch (error) {
        fail(`metadata.json is not valid JSON: ${error.message}`);
    }
}

if (metadata) {
    equal(metadata.uuid, 'dimmer@dimmer.linux', 'uuid');
    check(typeof metadata.name === 'string' && metadata.name.length > 0, 'name is set');
    check(typeof metadata.description === 'string' && metadata.description.length > 0,
        'description is set');
    equal(metadata['shell-version'], EXPECTED_SHELL_VERSIONS, 'shell-version');
    check(Number.isInteger(metadata.version) &&
        metadata.version >= MINIMUM_EXTENSION_VERSION,
    `version is at least ${MINIMUM_EXTENSION_VERSION}`);
}

if (failures > 0) {
    printerr(`metadata: ${failures} failure(s)`);
    imports.system.exit(1);
}

print('metadata: all checks passed');
