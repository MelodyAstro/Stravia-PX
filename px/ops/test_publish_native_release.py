"""Offline source binding and asset tamper checks for native publication."""
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('native_release', Path(__file__).with_name('publish-native-release.py'))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)
SOURCE = '1' * 40


class ReleaseTests(unittest.TestCase):
    def fixture(self, directory):
        payload = {name: name.encode() for name in release.COMPONENTS | {'stravia-server', 'source.patch'}}
        manifest = {'source_commit': SOURCE, 'upstream_commit': release.BASE, 'upstream_tag': 'v0.6.5',
            'source_patch_validation': {'source_tree': 'tree', 'reconstructed_tree': 'tree'},
            'files': {name: {'sha256': release.hashlib.sha256(data).hexdigest(), 'bytes': len(data)} for name, data in payload.items()}}
        (directory / 'build-manifest.json').write_text(json.dumps(manifest))
        (directory / 'source.patch').write_bytes(payload['source.patch'])
        with tarfile.open(directory / release.PACKAGE, 'w:gz') as archive:
            for name, data in {**payload, 'build-manifest.json': (directory / 'build-manifest.json').read_bytes()}.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))
        (directory / 'SHA256SUMS').write_text(''.join(release.digest(directory / name) + '  ' + name + '\n' for name in [release.PACKAGE, 'build-manifest.json', 'source.patch']))
        layers = directory / 'patches'
        layers.mkdir()
        for name in release.LAYERS:
            (layers / name).write_text(name)
        (layers / 'layers.json').write_text(json.dumps({'source_commit': SOURCE, 'base_commit': release.BASE, 'validation': {'source_tree': 'tree', 'reconstructed_tree': 'tree'}, 'layers': [{'file': name, 'sha256': release.digest(layers / name)} for name in sorted(release.LAYERS)]}))

    def test_prepares_all_checked_release_assets_without_network(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            self.fixture(directory)
            output = directory / 'output'
            report = release.prepare(directory, output, SOURCE)
            self.assertEqual(report['source_commit'], SOURCE)
            self.assertEqual(len(list(output.glob('*.wasm'))), 6)
            self.assertEqual(len(list(output.glob('*.patch'))), 5)
            self.assertTrue((output / 'SHA256SUMS').exists())

    def test_rejects_changed_source_and_tampered_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            self.fixture(directory)
            with self.assertRaisesRegex(ValueError, 'provenance'):
                release.prepare(directory, directory / 'wrong-source', '2' * 40)
            (directory / 'source.patch').write_text('modified')
            with self.assertRaisesRegex(ValueError, 'checksum'):
                release.prepare(directory, directory / 'tampered', SOURCE)


if __name__ == '__main__':
    unittest.main()
