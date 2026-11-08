#!/usr/bin/env python3
"""Stage a source-bound native package and preserve the running instance.

Run on the deployment host as root. --dry-run performs read-only package,
current-release and immutable-asset checks; it never calls the management API.
"""
import argparse
import contextlib
import gzip
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import sqlite3
import subprocess
import tarfile
import tempfile
import time
import urllib.request


ROOT = Path('/opt/stravia')
DATA = Path('/var/lib/stravia')
STATIC = Path('/var/www/stravia/_app/immutable')
MAINT = Path('/usr/local/sbin/stravia-maint')
REQUIRED_FILES = {'stravia-server', 'base.wasm', 'codex.wasm',
                  'antigravity.wasm', 'source.patch', 'build-manifest.json'}
OPTIONAL_FILES = {'policy-tools.wasm', 'claudecode.wasm'}
VENDORS = {'base': 'base.wasm', 'openai-codex': 'codex.wasm',
           'antigravity': 'antigravity.wasm'}
OPTIONAL_VENDORS = {'claude-code': 'claudecode.wasm'}
MAX_MEMBER = 300 * 1024 * 1024
MAX_TOTAL = 800 * 1024 * 1024
FEATURE = 'platform_plugins_v1'


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def digest(path):
    checksum = hashlib.sha256()
    with Path(path).open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            checksum.update(block)
    return checksum.hexdigest()


def read_json(path):
    return json.loads(Path(path).read_text())


def validate_spec(spec):
    for key in ['package_sha256', 'build_manifest_sha256', 'previous_host_sha256']:
        require(re.fullmatch('[0-9a-f]{64}', spec.get(key, '')),
                'A pinned SHA256 is required for ' + key)
    require(re.fullmatch('[0-9a-f]{40}', spec.get('source_commit', '')),
            'source_commit must be a complete commit id')
    require(re.fullmatch('[A-Za-z0-9][A-Za-z0-9._-]{0,99}',
                         spec.get('release_name', '')),
            'release_name must be one independent directory name')
    require(Path(spec.get('package_path', '')).is_absolute(),
            'package_path must be absolute')
    versions = spec.get('vendor_versions', {})
    require(set(VENDORS) <= set(versions) <= set(VENDORS | OPTIONAL_VENDORS) and all(
        isinstance(version, str) and version for version in versions.values()),
        'vendor_versions must pin base, openai-codex and antigravity; claude-code is optional')


def deployment_vendors(spec, inspected, installed):
    """Upgrade optional components only when already installed and explicitly pinned."""
    vendors = dict(VENDORS)
    for vendor, filename in OPTIONAL_VENDORS.items():
        pinned = vendor in spec['vendor_versions']
        available = filename in inspected['files']
        if pinned:
            require(available, 'Pinned Vendor component is absent from the package: ' + vendor)
            require(vendor in installed, 'Optional Vendor must already be installed: ' + vendor)
            vendors[vendor] = filename
        elif available and vendor in installed:
            require(False, 'Pin the installed optional Vendor version before upgrading: ' + vendor)
    return vendors


def member_name(member):
    path = PurePosixPath(member.name)
    require(member.name and len(member.name) <= 1024 and not path.is_absolute() and '..' not in path.parts
            and '\\' not in member.name and path.parts,
            'Unsafe package member path')
    require(member.isfile() or member.isdir(),
            'Package links, devices and special files are forbidden')
    name = str(path)
    if member.isdir():
        require(path.parts[0] == 'immutable', 'Unexpected package directory')
    elif len(path.parts) == 1:
        require(name in REQUIRED_FILES | OPTIONAL_FILES,
                'Unexpected root package file: ' + name)
    else:
        require(path.parts[0] == 'immutable', 'Unexpected package file tree')
    limit = MAX_MEMBER if name == 'stravia-server' else 64 * 1024 * 1024
    if name == 'build-manifest.json':
        limit = 1024 * 1024
    require(0 <= member.size <= limit, 'Package member exceeds size limit')
    return name


def inspect_package(package, expected_sha256, expected_manifest_sha256,
                    source_commit, required_features):
    """Read and verify every byte before any deployment destination is written."""
    require(package.is_file() and not package.is_symlink(), 'Package is not a regular file')
    require(digest(package) == expected_sha256, 'Package SHA256 mismatch')
    files, names, manifest_bytes, total = {}, set(), None, 0
    with tarfile.open(package, 'r:gz') as archive:
        for member in archive:
            name = member_name(member)
            require(name not in names, 'Duplicate package member: ' + name)
            require(len(names) < 10000, 'Package contains too many members')
            names.add(name)
            total += member.size
            require(total <= MAX_TOTAL, 'Package exceeds total uncompressed limit')
            if member.isdir():
                continue
            checksum = hashlib.sha256()
            with archive.extractfile(member) as source:
                content = bytearray() if name == 'build-manifest.json' else None
                size = 0
                for block in iter(lambda: source.read(1024 * 1024), b''):
                    size += len(block)
                    require(size <= member.size, 'Package member size mismatch')
                    checksum.update(block)
                    if content is not None:
                        content.extend(block)
                require(size == member.size, 'Truncated package member')
            files[name] = {'sha256': checksum.hexdigest(), 'bytes': size}
            if content is not None:
                manifest_bytes = bytes(content)
    roots = {name for name in files if '/' not in name}
    require(REQUIRED_FILES <= roots, 'Package lacks required native files')
    require(files['build-manifest.json']['sha256'] == expected_manifest_sha256,
            'Build manifest SHA256 mismatch')
    manifest = json.loads(manifest_bytes)
    require(manifest.get('source_commit') == source_commit, 'Package source commit mismatch')
    require(re.fullmatch('[0-9a-f]{40}', manifest.get('source_base_commit', '')) and
            isinstance(manifest.get('built_at_utc'), str) and manifest['built_at_utc'],
            'Package lacks source-base or build provenance')
    require(manifest.get('build_target') == 'x86_64-unknown-linux-gnu',
            'Package is not a native x86_64 GNU build')
    require(re.fullmatch(r'v\d+\.\d+\.\d+', manifest.get('upstream_tag', '')),
            'Package has no pinned stable upstream tag')
    require(set(required_features) <= set(manifest.get('features', [])),
            'Package drops required managed features')
    if 'outbound_identity_tls_v1' in manifest.get('features', []):
        require('claudecode.wasm' in roots, 'Identity package lacks the Claude Code component')
    require(set(manifest.get('files', {})) == roots - {'build-manifest.json'},
            'Build manifest must describe every packaged root file')
    for name, expected in manifest['files'].items():
        require(files[name] == expected, 'Manifest file digest or size mismatch: ' + name)
    assets = {name: record for name, record in files.items() if name.startswith('immutable/')}
    require(assets and len(assets) == manifest.get('immutable_asset_count'),
            'Immutable asset count mismatch')
    return {'manifest': manifest, 'files': files, 'assets': assets,
            'uncompressed_bytes': total}


def destination(root, relative):
    path = root / relative
    require(path.resolve().is_relative_to(root.resolve()) and not path.is_symlink(),
            'Deployment destination escapes its root or is a symlink')
    return path


def check_immutable_assets(inspected, static_root):
    for name, record in inspected['assets'].items():
        path = destination(static_root, PurePosixPath(name).relative_to('immutable'))
        if path.exists():
            require(path.is_file() and digest(path) == record['sha256'],
                    'Immutable asset collision: ' + name)
        compressed = path.with_name(path.name + '.gz')
        require(not compressed.is_symlink(), 'Compressed immutable asset is a symlink')
        if compressed.exists():
            require(compressed.is_file(), 'Compressed immutable asset is not a file')
            checksum = hashlib.sha256()
            expanded = 0
            with gzip.open(compressed, 'rb') as source:
                for block in iter(lambda: source.read(1024 * 1024), b''):
                    expanded += len(block)
                    require(expanded <= record['bytes'], 'Compressed immutable asset collision')
                    checksum.update(block)
            require(expanded == record['bytes'] and checksum.hexdigest() == record['sha256'],
                    'Compressed immutable asset collision: ' + name)


def atomic_write(path, body, mode=0o600):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as output:
        temporary = Path(output.name)
        try:
            output.write(body)
            output.flush()
            os.chmod(temporary, mode)
            os.fsync(output.fileno())
        except BaseException:
            temporary.unlink(missing_ok=True)
            raise
    try:
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def stage_package(package, inspected, candidate, static_root):
    # Reject an existing release, including a current-release alias. A failed
    # attempt remains available for diagnosis and is never overwritten.
    require(not candidate.exists() and not candidate.is_symlink(),
            'Candidate release already exists; choose a new release_name')
    check_immutable_assets(inspected, static_root)
    candidate.mkdir(parents=True, mode=0o755)
    candidate.chmod(0o755)
    seen = set()
    with tarfile.open(package, 'r:gz') as archive:
        for member in archive:
            name = member_name(member)
            require(name not in seen, 'Duplicate package member while staging')
            seen.add(name)
            if member.isdir():
                continue
            record = inspected['files'].get(name)
            require(record and member.size == record['bytes'], 'Package changed after inspection')
            if name.startswith('immutable/'):
                path = destination(static_root, PurePosixPath(name).relative_to('immutable'))
                path.parent.mkdir(parents=True, exist_ok=True, mode=0o755)
                parent = path.parent
                while parent.is_relative_to(static_root):
                    parent.chmod(0o755)
                    if parent == static_root:
                        break
                    parent = parent.parent
            else:
                path = destination(candidate, name)
            mode = 0o755 if name == 'stravia-server' else 0o644 if name.startswith('immutable/') else 0o600
            with archive.extractfile(member) as source:
                stage_member(source, path, record, mode, name.startswith('immutable/'))
            if name.startswith('immutable/') and path.suffix in ['.js', '.css', '.svg', '.wasm']:
                compressed = path.with_name(path.name + '.gz')
                require(not compressed.is_symlink(), 'Compressed immutable destination is a symlink')
                if not compressed.exists():
                    with tempfile.NamedTemporaryFile(dir=compressed.parent, delete=False) as output:
                        temporary = Path(output.name)
                        try:
                            with path.open('rb') as raw, gzip.GzipFile(fileobj=output, mode='wb', filename='', compresslevel=6, mtime=0) as zipped:
                                shutil.copyfileobj(raw, zipped, length=1024 * 1024)
                            output.flush()
                            os.chmod(temporary, 0o644)
                            os.fsync(output.fileno())
                        except BaseException:
                            temporary.unlink(missing_ok=True)
                            raise
                    try:
                        os.replace(temporary, compressed)
                    finally:
                        temporary.unlink(missing_ok=True)
    require({name for name in seen if name in inspected['files']} == set(inspected['files']),
            'Package member set changed after inspection')


def stage_member(source, path, record, mode, immutable):
    existing = immutable and path.exists()
    if existing:
        require(path.is_file() and digest(path) == record['sha256'],
                'Immutable asset changed during staging')
    temporary = None
    try:
        output = tempfile.NamedTemporaryFile(dir=path.parent, delete=False) if not existing else None
        if output is not None:
            temporary = Path(output.name)
        checksum, size = hashlib.sha256(), 0
        try:
            for block in iter(lambda: source.read(1024 * 1024), b''):
                size += len(block)
                require(size <= record['bytes'], 'Package grew after inspection')
                checksum.update(block)
                if output is not None:
                    output.write(block)
            require(size == record['bytes'] and checksum.hexdigest() == record['sha256'],
                    'Package content changed after inspection')
            if output is not None:
                output.flush()
                os.chmod(temporary, mode)
                os.fsync(output.fileno())
        finally:
            if output is not None:
                output.close()
        if temporary is not None:
            os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def disk_plan(spec, inspected, static_root):
    candidate_bytes = sum(record['bytes'] for name, record in inspected['files'].items()
                          if not name.startswith('immutable/'))
    new_assets, gzip_reserve = 0, 0
    for name, record in inspected['assets'].items():
        path = destination(static_root, PurePosixPath(name).relative_to('immutable'))
        if not path.exists():
            new_assets += record['bytes']
        if path.suffix in ['.js', '.css', '.svg', '.wasm'] and not path.with_name(path.name + '.gz').exists():
            gzip_reserve += record['bytes'] * 102 // 100 + 1024
    backup_reserve = spec.get('backup_reserve_bytes')
    data_bytes, data_files = 0, 0
    for path in DATA.rglob('*'):
        if path.is_file() and not path.is_symlink():
            data_bytes += path.stat().st_size
            data_files += 1
    # restore retains the newer DATA tree and then extracts the old tree.
    # Compressed backup space cannot also pay for that second unpacked copy.
    rollback_reserve = data_bytes * 102 // 100 + data_files * 1024 + 10 * 1024 * 1024
    if backup_reserve is None:
        raw, count = 0, 0
        for tree in [DATA, ROOT / 'compat', ROOT / 'responses-compat']:
            for path in tree.rglob('*'):
                if 'staged' in path.relative_to(tree).parts and tree != DATA:
                    continue
                if path.is_file() and not path.is_symlink():
                    raw += path.stat().st_size
                    count += 1
        backup_reserve = raw * 102 // 100 + count * 1024 + 10 * 1024 * 1024
    margin = spec.get('free_space_margin_bytes', 128 * 1024 * 1024)
    require(isinstance(backup_reserve, int) and backup_reserve > 0 and
            isinstance(margin, int) and margin >= 32 * 1024 * 1024,
            'Backup reserve must be positive and free-space margin at least 32 MiB')
    return {'candidate_bytes': candidate_bytes, 'new_asset_bytes': new_assets,
            'gzip_reserve_bytes': gzip_reserve, 'backup_reserve_bytes': backup_reserve,
            'rollback_reserve_bytes': rollback_reserve,
            'free_space_margin_bytes': margin,
            'required_free_bytes': candidate_bytes + new_assets + gzip_reserve + backup_reserve + rollback_reserve + margin}


def current_release(spec, manifest):
    previous = (ROOT / 'current').resolve()
    require(previous.is_relative_to((ROOT / 'releases').resolve()),
            'Current release is outside the managed release tree')
    require(digest(previous / 'stravia-server') == spec['previous_host_sha256'],
            'Current host differs from the pinned previous host')
    responses = read_json(ROOT / 'responses-compat/manifest.json')
    ga = read_json(ROOT / 'compat/manifest.json')
    require(responses['host_sha256'] == spec['previous_host_sha256'],
            'Active Responses manifest does not match current host')
    require(manifest['upstream_tag'] == responses['upstream_tag'],
            'This script only replaces a build on the same upstream version')
    require(set(responses.get('features', [])) <= set(manifest['features']),
            'Candidate would discard an active managed feature')
    for directory, metadata in [('responses-compat', responses), ('compat', ga)]:
        component = metadata['component']
        require(Path(component).name == component, 'Unsafe managed component filename')
        require(digest(ROOT / directory / component) == metadata['sha256'],
                'Active managed component digest mismatch')
    return previous, responses, ga


def configuration_fingerprint():
    result = {}
    with sqlite3.connect('file:' + str(DATA / 'db/gateway.db') + '?mode=ro', uri=True) as db:
        require(db.execute('PRAGMA quick_check').fetchone()[0] == 'ok', 'SQLite integrity check failed')
        require(not db.execute('PRAGMA foreign_key_check').fetchall(), 'SQLite foreign key check failed')
        for table in ['api_keys', 'api_key_models', 'models', 'model_backends']:
            columns = [row[1] for row in db.execute('PRAGMA table_info(' + table + ')')
                       if row[1] not in ['last_used_at', 'created_at', 'updated_at']]
            require(columns, 'Missing expected configuration table: ' + table)
            rows = db.execute('SELECT ' + ','.join('"' + name + '"' for name in columns)
                              + ' FROM ' + table + ' ORDER BY 1').fetchall()
            result[table] = hashlib.sha256(json.dumps(rows, sort_keys=True, default=str).encode()).hexdigest()
        rows = db.execute("SELECT name,value FROM settings WHERE name LIKE 'px_request_models:%' ORDER BY name").fetchall()
        result['request_models'] = hashlib.sha256(json.dumps(rows).encode()).hexdigest()
    installed = DATA / 'platform-plugins/installed.json'
    result['platform_plugins'] = digest(installed) if installed.exists() else None
    return result


def vendor_digests():
    with sqlite3.connect('file:' + str(DATA / 'db/gateway.db') + '?mode=ro', uri=True) as db:
        return dict(db.execute('SELECT vendor_id,digest FROM vendor_plugins'))


def client():
    module_spec = importlib.util.spec_from_file_location('stravia_deploy_client', '/root/stravia-client.py')
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    return module.Client().login()


def provider_invariants(api):
    providers = api.call('GET', '/api/v1/providers')['data']
    return {provider['id']: {
        'enabled': provider.get('is_enabled'),
        'credential_status': provider.get('credential_status'),
        'claude_code_compat': provider.get('vendor_options', {}).get('claude_code_compat'),
        'websocket_url': provider.get('vendor_options', {}).get('websocket_url'),
    } for provider in providers}


def ready(timeout=180):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen('http://127.0.0.1:23471/readyz', timeout=3) as response:
                if response.status == 200:
                    return
        except OSError:
            pass
        time.sleep(1)
    raise RuntimeError('Native service readiness timed out')


def run(*arguments):
    return subprocess.run([str(argument) for argument in arguments], check=True,
                          capture_output=True, text=True)


def check_gnu(candidate, upstream_tag):
    loader = ROOT / 'runtime/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2'
    libraries = loader.parent
    linked = run(loader, '--library-path', libraries, '--list', candidate / 'stravia-server')
    require('not found' not in linked.stdout and 'musl' not in linked.stdout.lower(),
            'Candidate cannot use the isolated GNU runtime')
    for dependency in re.findall(r'=>\s+(/\S+)', linked.stdout):
        require(Path(dependency).resolve().is_relative_to((ROOT / 'runtime').resolve()),
                'Candidate resolves a dependency outside the isolated GNU runtime')
    version = run(loader, '--library-path', libraries, candidate / 'stravia-server', '--version')
    require(upstream_tag.removeprefix('v') in version.stdout, 'Unexpected native server version')


@contextlib.contextmanager
def exclusive_lock(path):
    import fcntl
    with Path(path).open('a') as handle:
        fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield


def switch(candidate):
    temporary = ROOT / 'current.platform-plugins-next'
    temporary.unlink(missing_ok=True)
    temporary.symlink_to(candidate, target_is_directory=True)
    os.replace(temporary, ROOT / 'current')


def sync_managed_metadata(spec, inspected, responses, ga, changed):
    manifest, files = inspected['manifest'], inspected['files']
    source_fields = {key: manifest[key] for key in ['source_commit', 'source_base_commit', 'upstream_tag', 'built_at_utc']}
    source_fields['source_patch_sha256'] = files['source.patch']['sha256']
    candidate = ROOT / 'releases' / manifest['upstream_tag'] / spec['release_name']
    patch = (candidate / 'source.patch').read_bytes()
    directory = ROOT / 'responses-compat'
    atomic_write(directory / responses['component'], (candidate / 'codex.wasm').read_bytes())
    atomic_write(directory / 'source.patch', patch)
    responses = {**responses, **source_fields, 'host_sha256': files['stravia-server']['sha256'],
                 'base_component_sha256': files['base.wasm']['sha256'],
                 'sha256': files['codex.wasm']['sha256'],
                 'version': spec['vendor_versions']['openai-codex'], 'features': manifest['features']}
    atomic_write(directory / 'manifest.json', (json.dumps(responses, indent=2) + '\n').encode())
    if 'antigravity' in changed:
        directory = ROOT / 'compat'
        atomic_write(directory / ga['component'], (candidate / 'antigravity.wasm').read_bytes())
        atomic_write(directory / 'source.patch', patch)
        ga = {**ga, **source_fields, 'sha256': files['antigravity.wasm']['sha256'],
              'version': spec['vendor_versions']['antigravity']}
        atomic_write(directory / 'manifest.json', (json.dumps(ga, indent=2) + '\n').encode())


def deploy(spec, inspected):
    manifest, files = inspected['manifest'], inspected['files']
    previous, responses, ga = current_release(spec, manifest)
    candidate = ROOT / 'releases' / manifest['upstream_tag'] / spec['release_name']
    require(not candidate.exists() and candidate.resolve() != previous,
            'A new release directory is required')
    api = client()
    plugins = api.call('GET', '/api/v1/vendor-plugins')['data']
    require(all(plugin['status'] == 'ready' for plugin in plugins), 'An existing Vendor is unavailable')
    before_versions = {plugin['vendor_id']: plugin['version'] for plugin in plugins}
    require(set(VENDORS) <= set(before_versions), 'Required managed Vendors are not installed')
    managed_vendors = deployment_vendors(spec, inspected, before_versions)
    before_digests = vendor_digests()
    require(before_digests['openai-codex'] == responses['sha256'] and
            before_digests['antigravity'] == ga['sha256'], 'Installed Vendor and maintenance digests differ')
    before = configuration_fingerprint()
    providers = provider_invariants(api)
    require(not api.call('GET', '/api/v1/observations/debug')['data']['enabled'],
            'Disable temporary debugging before deployment')
    nginx = {str(path): digest(path) for path in [Path('/etc/nginx/http.d/stravia.conf'), Path('/etc/nginx/stravia-proxy.inc')]}
    space = disk_plan(spec, inspected, STATIC)
    require(shutil.disk_usage(ROOT).free > space['required_free_bytes'], 'Insufficient staging and full-backup space')
    stage_package(Path(spec['package_path']), inspected, candidate, STATIC)
    check_gnu(candidate, manifest['upstream_tag'])
    backup_result = run(MAINT, 'backup')
    backup = next(json.loads(line) for line in reversed(backup_result.stdout.splitlines())
                  if line.startswith('{') and 'archive' in json.loads(line))
    require(Path(backup['release']).resolve() == previous and digest(backup['archive']) == backup['sha256'],
            'Backup does not match the previous release')
    changed = []
    cutover_started = False
    try:
        # stravia-maint acquires this same lock. Acquire it only after its backup
        # subprocess returns, and release it before invoking restore on failure.
        with exclusive_lock('/run/stravia-maint.lock'):
            require((ROOT / 'current').resolve() == previous and
                    digest(previous / 'stravia-server') == spec['previous_host_sha256'],
                    'Active release changed during staging')
            require(configuration_fingerprint() == before, 'Configuration changed during staging')
            cutover_started = True
            run('rc-service', 'stravia', 'stop')
            switch(candidate)
            run('rc-service', 'stravia', 'start')
            ready()
            api = client()
            for vendor, name in managed_vendors.items():
                if vendor == 'base':  # The host loads its matching embedded Base component.
                    continue
                if before_digests[vendor] == files[name]['sha256']:
                    continue
                preview = api.call('POST', '/api/v1/vendor-plugins/import',
                                   (candidate / name).read_bytes(), 'application/wasm', timeout=600)['data']
                require(preview['vendor_id'] == vendor and preview['new_version'] == spec['vendor_versions'][vendor],
                        'Candidate Vendor identity or version mismatch')
                api.call('POST', '/api/v1/vendor-plugins/confirm',
                         {'preview_id': preview['id'], 'allow_data_discard': False}, timeout=600)
                changed.append(vendor)
            plugins = api.call('GET', '/api/v1/vendor-plugins')['data']
            versions = {plugin['vendor_id']: plugin['version'] for plugin in plugins}
            require(all(plugin['status'] == 'ready' for plugin in plugins) and set(versions) == set(before_versions),
                    'Vendor inventory changed or a Vendor is unavailable')
            for vendor, version in versions.items():
                require(version == spec['vendor_versions'].get(vendor, before_versions[vendor]),
                        'Vendor version differs from its pinned version: ' + vendor)
            after_digests = vendor_digests()
            for vendor, old_digest in before_digests.items():
                expected = files[managed_vendors[vendor]]['sha256'] if vendor in managed_vendors else old_digest
                require(after_digests.get(vendor) == expected, 'Installed Vendor digest mismatch: ' + vendor)
            require(api.call('GET', '/api/v1/platform-plugins')['data'] is not None,
                    'Platform plugin administration is unavailable')
            require(provider_invariants(api) == providers, 'Provider enablement or compatibility options changed')
            require(not api.call('GET', '/api/v1/observations/debug')['data']['enabled'], 'Debug mode changed')
            require(configuration_fingerprint() == before, 'Keys, routes, mappings or platform settings changed')
            require(all(digest(path) == checksum for path, checksum in nginx.items()), 'Nginx configuration changed')
            sync_managed_metadata(spec, inspected, responses, ga, changed)
            ready()
            report = {'deployed': True, 'source_commit': spec['source_commit'],
                      'host_sha256': files['stravia-server']['sha256'], 'previous_release': str(previous),
                      'current_release': str(candidate), 'features': manifest['features'],
                      'vendor_versions': versions, 'reimported_vendors': changed, 'backup': backup,
                      'configuration_preserved': True, 'nginx_preserved': True, 'debug_off': True,
                      'immutable_asset_count': len(inspected['assets']), 'disk_plan': space,
                      'disk_free_bytes': shutil.disk_usage(ROOT).free}
            atomic_write(Path(spec.get('report_path', '/root/stravia-platform-plugins-deployment.json')),
                         (json.dumps(report, indent=2) + '\n').encode())
    except BaseException:
        # Restore the complete data snapshot and matching old program, never
        # just the binary pointer. The maintenance helper retains newer data.
        if cutover_started:
            run(MAINT, 'restore', backup['archive'])
        raise
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--spec', type=Path, required=True)
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    spec = read_json(args.spec)
    validate_spec(spec)
    inspected = inspect_package(Path(spec['package_path']), spec['package_sha256'],
                                spec['build_manifest_sha256'], spec['source_commit'],
                                set(spec.get('required_features', [])) | {FEATURE})
    managed_vendors = deployment_vendors(spec, inspected, vendor_digests())
    previous, _, _ = current_release(spec, inspected['manifest'])
    candidate = ROOT / 'releases' / inspected['manifest']['upstream_tag'] / spec['release_name']
    require(not candidate.exists() and not candidate.is_symlink(), 'Candidate release already exists')
    check_immutable_assets(inspected, STATIC)
    space = disk_plan(spec, inspected, STATIC)
    if args.dry_run:
        print(json.dumps({'preflight_passed': True, 'read_only': True, 'source_commit': spec['source_commit'],
                          'previous_release': str(previous), 'candidate': str(candidate),
                          'disk_plan': space, 'disk_free_bytes': shutil.disk_usage(ROOT).free,
                          'managed_vendors': list(managed_vendors),
                          'immutable_asset_count': len(inspected['assets'])}))
        return
    os.umask(0o077)
    require(os.geteuid() == 0, 'Deployment must run as root')
    with exclusive_lock('/run/stravia-platform-deploy.lock'):
        print(json.dumps(deploy(spec, inspected)), flush=True)


if __name__ == '__main__':
    main()
