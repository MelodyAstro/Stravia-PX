"""Publish only checksum-verified assets from a successful, source-matching native run."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import urllib.request
import urllib.error
import zipfile

TAG = 'v0.6.5-patch'
BASE = '42348db6d7fbad32f9bcf56d94fded8a2ec17ec4'
PACKAGE = 'strinova-v0.6.5-linux-x86_64-gnu.tar.gz'
COMPONENTS = {'base.wasm', 'codex.wasm', 'antigravity.wasm', 'claudecode.wasm', 'model-mapping.wasm', 'policy-tools.wasm'}
LAYERS = {'platform-host.patch', 'vendor-compatibility.patch', 'webui.patch', 'maintenance.patch'}


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        while block := stream.read(1024 * 1024):
            value.update(block)
    return value.hexdigest()


def api(path, method='GET', data=None):
    request = urllib.request.Request('https://api.github.com/repos/' + os.environ['GITHUB_REPOSITORY'] + path,
        method=method, data=None if data is None else json.dumps(data).encode(),
        headers={'Authorization': 'Bearer ' + os.environ['GH_TOKEN'], 'Accept': 'application/vnd.github+json', 'Content-Type': 'application/json', 'X-GitHub-Api-Version': '2022-11-28'})
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def verify_run():
    run_id = os.environ['NATIVE_RUN_ID']
    if not run_id.isdigit():
        raise ValueError('A numeric run ID is required')
    run = api('/actions/runs/' + run_id)
    if run['status'] != 'completed' or run['conclusion'] != 'success' or run['head_sha'] != os.environ['GITHUB_SHA'] or run['path'].split('@')[0] != '.github/workflows/px-native-linux.yml':
        raise ValueError('Native run must have passed for this exact main commit')
    print(json.dumps({'verified_run': int(run_id), 'source_commit': run['head_sha']}))


def prepare(directory, output, source):
    manifest = json.loads((directory / 'build-manifest.json').read_text())
    if manifest['source_commit'] != source or manifest['upstream_commit'] != BASE or manifest['upstream_tag'] != 'v0.6.5':
        raise ValueError('Native package provenance mismatch')
    if manifest['source_patch_validation']['source_tree'] != manifest['source_patch_validation']['reconstructed_tree']:
        raise ValueError('Source patch tree mismatch')
    expected = {PACKAGE, 'build-manifest.json', 'source.patch'}
    checks = {}
    for line in (directory / 'SHA256SUMS').read_text().splitlines():
        checksum, name = line.split(maxsplit=1)
        if name not in expected or name in checks or not re.fullmatch('[a-f0-9]{64}', checksum) or digest(directory / name) != checksum:
            raise ValueError('Native artifact checksum mismatch')
        checks[name] = checksum
    if set(checks) != expected:
        raise ValueError('Incomplete native artifact checksums')
    output.mkdir(parents=True, exist_ok=False)
    for name in expected:
        (output / name).write_bytes((directory / name).read_bytes())
    (output / 'CI-SHA256SUMS').write_bytes((directory / 'SHA256SUMS').read_bytes())
    with tarfile.open(directory / PACKAGE, 'r:gz') as archive:
        records = {member.name: member for member in archive.getmembers()}
        if len(records) != len(archive.getmembers()):
            raise ValueError('Duplicate archive entry')
        if set(manifest['files']) != COMPONENTS | {'stravia-server', 'source.patch'}:
            raise ValueError('Unexpected native package members')
        for name in manifest['files']:
            record = records[name]
            if not record.isfile() or record.size != manifest['files'][name]['bytes'] or record.size > 1024 * 1024 * 1024:
                raise ValueError('Invalid native archive entry')
            checksum = hashlib.sha256()
            with archive.extractfile(record) as stream:
                while block := stream.read(1024 * 1024):
                    checksum.update(block)
            if checksum.hexdigest() != manifest['files'][name]['sha256']:
                raise ValueError('Native member checksum mismatch')
            if name in COMPONENTS:
                if record.size > 64 * 1024 * 1024:
                    raise ValueError('Component exceeds release size limit')
                (output / name).write_bytes(archive.extractfile(record).read())
        if archive.extractfile(records['build-manifest.json']).read() != (directory / 'build-manifest.json').read_bytes():
            raise ValueError('Embedded manifest mismatch')
    layers = json.loads((directory / 'patches/layers.json').read_text())
    if layers['base_commit'] != BASE or layers['source_commit'] != source or layers['validation']['source_tree'] != layers['validation']['reconstructed_tree']:
        raise ValueError('Layer provenance mismatch')
    if {layer['file'] for layer in layers['layers']} != LAYERS or len(layers['layers']) != len(LAYERS):
        raise ValueError('Incomplete source patch layers')
    for layer in layers['layers']:
        path = directory / 'patches' / layer['file']
        if digest(path) != layer['sha256']:
            raise ValueError('Layer checksum mismatch')
        (output / path.name).write_bytes(path.read_bytes())
    (output / 'layers.json').write_bytes((directory / 'patches/layers.json').read_bytes())
    with zipfile.ZipFile(output / ('strinova-' + TAG + '-plugins.zip'), 'w', zipfile.ZIP_DEFLATED) as bundle:
        for name in sorted(COMPONENTS | {'build-manifest.json'}):
            bundle.write(output / name, name)
    (output / 'SHA256SUMS').write_text(''.join(digest(path) + '  ' + path.name + '\n' for path in sorted(output.iterdir())), encoding='utf-8')
    return {'source_commit': source, 'files': len(list(output.iterdir()))}


def publish(output):
    source = os.environ['GITHUB_SHA']
    manifest = json.loads((output / 'build-manifest.json').read_text())
    if manifest['source_commit'] != source:
        raise ValueError('Prepared release source mismatch')
    for line in (output / 'SHA256SUMS').read_text().splitlines():
        checksum, name = line.split(maxsplit=1)
        if Path(name).name != name or digest(output / name) != checksum:
            raise ValueError('Prepared release was modified')
    repo = os.environ['GITHUB_REPOSITORY']
    # Creating tags with GITHUB_TOKEN prevents recursively starting upstream's
    # desktop/container release matrix in this native-only fork publication.
    existing = subprocess.run(['gh', 'release', 'view', TAG, '--repo', repo, '--json', 'isDraft,targetCommitish'], capture_output=True, text=True)
    if existing.returncode == 0:
        data = json.loads(existing.stdout)
        if not data['isDraft'] or data['targetCommitish'] != source:
            raise ValueError('Refusing to replace an existing published or mismatched release')
    try:
        tag = api('/git/ref/tags/' + TAG)
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
        tag = api('/git/refs', 'POST', {'ref': 'refs/tags/' + TAG, 'sha': source})
    if tag['object']['type'] != 'commit' or tag['object']['sha'] != source:
        raise ValueError('Release tag points to another source commit')
    if existing.returncode != 0:
        subprocess.run(['gh', 'release', 'create', TAG, '--repo', repo, '--target', source, '--verify-tag', '--title', 'Strinova 0.6.5-patch', '--notes-file', 'px/RELEASE-v0.6.5-patch.md', '--draft'], check=True)
    subprocess.run(['gh', 'release', 'upload', TAG, '--repo', repo, '--clobber', *map(str, sorted(output.iterdir()))], check=True)
    subprocess.run(['gh', 'release', 'edit', TAG, '--repo', repo, '--draft=false', '--prerelease=false', '--latest'], check=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--verify-run', action='store_true')
    parser.add_argument('--artifact-dir', type=Path)
    parser.add_argument('--output-dir', type=Path, default=Path('release-assets'))
    parser.add_argument('--publish', action='store_true')
    args = parser.parse_args()
    if args.verify_run:
        verify_run()
    elif args.publish:
        publish(args.output_dir)
    else:
        print(json.dumps(prepare(args.artifact_dir, args.output_dir, os.environ['GITHUB_SHA'])))
