"""Export independently reviewable layers and prove they reconstruct the fork."""
import argparse
import json
from pathlib import Path
import subprocess
import importlib.util

spec = importlib.util.spec_from_file_location('native_package', Path(__file__).with_name('package-ci-native.py'))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)

def export(repo, output):
    commit = package.git(repo, 'rev-parse', 'HEAD').decode().strip()
    paths = package.git(repo, 'diff', '--name-only', '-z', package.BASE_COMMIT, commit).decode().split('\0')
    layers = {name: [] for name in ['platform-host', 'vendor-compatibility', 'webui', 'maintenance']}
    for path in filter(None, paths):
        if path.startswith('frontend/'):
            layer = 'webui'
        elif path.startswith(('backend/crates/stravia-vendor-codex/', 'backend/crates/stravia-vendor-antigravity/', 'backend/crates/stravia-vendor-claudecode/', 'backend/crates/stravia-vendor-common/')):
            layer = 'vendor-compatibility'
        elif path.startswith(('backend/', 'patches/', '.cargo/')) or path in ['Cargo.toml', 'Cargo.lock']:
            layer = 'platform-host'
        else:
            layer = 'maintenance'
        layers[layer].append(path)
    output.mkdir(parents=True, exist_ok=True)
    combined = bytearray()
    manifest = {'base_commit': package.BASE_COMMIT, 'source_commit': commit, 'layers': []}
    for name, members in layers.items():
        patch = package.git(repo, 'diff', '--binary', '--full-index', '--no-renames', '--no-color', package.BASE_COMMIT, commit, '--', *members) if members else b''
        file = output / (name + '.patch')
        file.write_bytes(patch)
        combined.extend(patch)
        manifest['layers'].append({'file': file.name, 'sha256': package.digest(file), 'paths': members})
    manifest['validation'] = package.verify_source_patch(repo, package.BASE_COMMIT, commit, bytes(combined))
    (output/'layers.json').write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({'source_commit':commit, 'reconstruction_verified': True, 'layers':len(layers)}))

if __name__ == '__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,default=Path('dist/px-native/patches'))
    args=p.parse_args()
    export(Path(__file__).resolve().parents[2],args.output)
