"""Exercise real package inspection and staging using isolated filesystem inputs."""
import gzip
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch


module_spec = importlib.util.spec_from_file_location(
    'platform_deploy', Path(__file__).with_name('deploy-platform-plugins.py'))
deploy = importlib.util.module_from_spec(module_spec)
module_spec.loader.exec_module(deploy)
COMMIT = '1' * 40


def make_package(directory, *, invalid_manifest=False, extra=None):
    files = {
        'stravia-server': b'isolated native executable fixture',
        'base.wasm': b'base component', 'codex.wasm': b'codex component',
        'antigravity.wasm': b'GA component', 'source.patch': b'complete source changes',
        'policy-tools.wasm': b'optional independent component',
        'immutable/entry/app.js': b'export const deployment = "verified";\n',
    }
    manifest = {
        'source_commit': COMMIT, 'source_base_commit': '2' * 40,
        'upstream_tag': 'v0.6.4', 'build_target': 'x86_64-unknown-linux-gnu',
        'built_at_utc': '2026-11-08T00:00:00+00:00',
        'features': [deploy.FEATURE, 'native_custom_tools_v1'],
        'immutable_asset_count': 1,
        'files': {name: {'sha256': hashlib.sha256(body).hexdigest(), 'bytes': len(body)}
                  for name, body in files.items() if '/' not in name},
    }
    if invalid_manifest:
        manifest['files']['codex.wasm']['sha256'] = '0' * 64
    files['build-manifest.json'] = json.dumps(manifest).encode()
    package = directory / 'native.tar.gz'
    with tarfile.open(package, 'w:gz') as archive:
        for name, body in files.items():
            member = tarfile.TarInfo(name)
            member.size = len(body)
            archive.addfile(member, io.BytesIO(body))
        if extra is not None:
            archive.addfile(extra)
    return package, files


def inspect(package, files):
    return deploy.inspect_package(
        package, deploy.digest(package),
        hashlib.sha256(files['build-manifest.json']).hexdigest(), COMMIT,
        {deploy.FEATURE, 'native_custom_tools_v1'})


class NativePackageTests(unittest.TestCase):
    def test_source_bound_package_stages_assets_without_overwriting_existing_content(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package, files = make_package(root)
            inspected = inspect(package, files)
            static = root / 'static'
            existing = static / 'entry/app.js'
            existing.parent.mkdir(parents=True)
            existing.write_bytes(files['immutable/entry/app.js'])
            before = existing.stat().st_mtime_ns
            candidate = root / 'releases/new-build'
            deploy.stage_package(package, inspected, candidate, static)
            self.assertEqual(existing.stat().st_mtime_ns, before)
            self.assertEqual(gzip.decompress(existing.with_suffix('.js.gz').read_bytes()), existing.read_bytes())
            for name, body in files.items():
                if '/' not in name:
                    self.assertEqual((candidate / name).read_bytes(), body)
            self.assertRaisesRegex(RuntimeError, 'already exists',
                                   deploy.stage_package, package, inspected, candidate, static)

    def test_immutable_collision_rejects_before_creating_candidate(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package, files = make_package(root)
            inspected = inspect(package, files)
            static = root / 'static'
            asset = static / 'entry/app.js'
            asset.parent.mkdir(parents=True)
            asset.write_bytes(b'previous immutable file with the same name')
            candidate = root / 'candidate'
            with self.assertRaisesRegex(RuntimeError, 'Immutable asset collision'):
                deploy.stage_package(package, inspected, candidate, static)
            self.assertFalse(candidate.exists())
            self.assertEqual(asset.read_bytes(), b'previous immutable file with the same name')

    def test_compressed_asset_collision_is_rejected_without_overwrite(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package, files = make_package(root)
            inspected = inspect(package, files)
            static = root / 'static'
            asset = static / 'entry/app.js'
            asset.parent.mkdir(parents=True)
            asset.write_bytes(files['immutable/entry/app.js'])
            compressed = asset.with_suffix('.js.gz')
            compressed.write_bytes(gzip.compress(b'wrong gzip contents'))
            with self.assertRaisesRegex(RuntimeError, 'Compressed immutable asset collision'):
                deploy.check_immutable_assets(inspected, static)
            self.assertEqual(gzip.decompress(compressed.read_bytes()), b'wrong gzip contents')

    def test_every_manifest_file_is_checked_even_when_archive_checksum_matches(self):
        with tempfile.TemporaryDirectory() as temporary:
            package, files = make_package(Path(temporary), invalid_manifest=True)
            with self.assertRaisesRegex(RuntimeError, 'digest or size mismatch: codex.wasm'):
                inspect(package, files)

    def test_paths_links_duplicates_and_oversized_members_are_rejected(self):
        cases = []
        for name in ['/absolute', 'immutable/../../outside']:
            member = tarfile.TarInfo(name)
            cases.append(member)
        for kind in [tarfile.SYMTYPE, tarfile.LNKTYPE]:
            member = tarfile.TarInfo('immutable/link')
            member.type, member.linkname = kind, '/outside'
            cases.append(member)
        cases.append(tarfile.TarInfo('base.wasm'))
        member = tarfile.TarInfo('stravia-server')
        member.size = deploy.MAX_MEMBER + 1
        cases.append(member)
        for member in cases:
            with self.subTest(member=member.name, kind=member.type), tempfile.TemporaryDirectory() as temporary:
                package, files = make_package(Path(temporary), extra=member)
                with self.assertRaises(RuntimeError):
                    inspect(package, files)

    def test_package_and_commit_pins_are_enforced(self):
        with tempfile.TemporaryDirectory() as temporary:
            package, files = make_package(Path(temporary))
            manifest_hash = hashlib.sha256(files['build-manifest.json']).hexdigest()
            with self.assertRaisesRegex(RuntimeError, 'Package SHA256 mismatch'):
                deploy.inspect_package(package, '0' * 64, manifest_hash, COMMIT, {deploy.FEATURE})
            with self.assertRaisesRegex(RuntimeError, 'source commit mismatch'):
                deploy.inspect_package(package, deploy.digest(package), manifest_hash, '3' * 40, {deploy.FEATURE})

    def test_disk_budget_reserves_a_second_uncompressed_data_tree_for_rollback(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package, files = make_package(root)
            inspected = inspect(package, files)
            data = root / 'data'
            (data / 'db').mkdir(parents=True)
            (data / 'platform-plugins/artifacts').mkdir(parents=True)
            (data / 'db/gateway.db').write_bytes(b'database page' * 8192)
            (data / 'platform-plugins/artifacts/component.wasm').write_bytes(b'component bytes' * 8192)
            logical_data_bytes = sum(path.stat().st_size for path in data.rglob('*') if path.is_file())
            # A compressed backup estimate is deliberately smaller than DATA:
            # rollback must still retain room for the full extracted tree.
            with patch.object(deploy, 'DATA', data):
                plan = deploy.disk_plan({'backup_reserve_bytes': 64 * 1024}, inspected, root / 'static')
            self.assertGreaterEqual(plan['rollback_reserve_bytes'], logical_data_bytes)
            other_costs = sum(plan[key] for key in ['candidate_bytes', 'new_asset_bytes',
                                                   'gzip_reserve_bytes', 'backup_reserve_bytes',
                                                   'free_space_margin_bytes'])
            self.assertGreaterEqual(plan['required_free_bytes'] - other_costs, logical_data_bytes)
            self.assertEqual(plan['backup_reserve_bytes'], 64 * 1024)


if __name__ == '__main__':
    unittest.main()
