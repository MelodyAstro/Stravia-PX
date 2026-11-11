"""Isolated source reconstruction and native-package provenance regressions."""

import contextlib
import hashlib
import importlib.util
import io
import json
import pathlib
import subprocess
import tarfile
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location("package_ci_native", pathlib.Path(__file__).with_name("package-ci-native.py"))
PACKAGER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGER)


class NativePackageTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="stravia-package-test-")
        self.addCleanup(self.directory.cleanup)
        self.repo = pathlib.Path(self.directory.name)
        self.git("init", "--quiet")
        self.git("config", "user.name", "Package test")
        self.git("config", "user.email", "package-test@localhost")
        self.git("config", "core.autocrlf", "false")
        self.write(".gitignore", b"target/\ndist/\nprivate/\nevidence/\n")
        self.write("Cargo.toml", b'[workspace.package]\nversion = "0.6.4"\n')
        self.write("Cargo.lock", b"old lock\n")
        self.write("Taskfile.yml", b"version: '3'\n")
        self.write(".cargo/config.toml", b"[build]\njobs = 1\n")
        self.write(".github/workflows/build.yml", b"name: Original build\n")
        self.write("backend/old.rs", b"old implementation\n")
        self.write("frontend/old.ts", b"old frontend\n")
        self.write("binary.fixture", b"\x00old\x01")
        self.commit()
        self.base = self.git("rev-parse", "HEAD").decode().strip()

    def git(self, *arguments):
        return subprocess.check_output(
            ["git", "-c", "safe.directory=" + self.repo.as_posix(), "-C", str(self.repo), *arguments],
            stderr=subprocess.PIPE,
        )

    def write(self, name, content):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)

    def commit(self):
        self.git("add", "--all")
        self.git("commit", "--quiet", "--allow-empty", "-m", "fixture")

    def change_source(self):
        self.write("Cargo.toml", b'[workspace.package]\nversion = "0.6.4"\n[workspace]\nresolver = "2"\n')
        self.write("Cargo.lock", b"new lock\n")
        self.write("Taskfile.yml", b"version: '3'\ntasks: {}\n")
        self.write(".cargo/config.toml", b"[build]\njobs = 2\n")
        self.write(".github/workflows/build.yml", b"name: Native build\n")
        (self.repo / "backend/old.rs").unlink()
        self.write("backend/new.rs", b"new implementation\n")
        self.write("frontend/old.ts", b"new frontend\n")
        self.write("px/README.md", b"Maintain the private package\n")
        self.write("px/ops/maintain.py", b"print('maintenance')\n")
        self.write(".env.integration.example", b"STRAVIA_PORT=23471\n")
        self.write("source with spaces/\u4f8b.txt", b"Tracked source\n")
        self.write("binary.fixture", b"\x00new\xff\x01")
        self.write(PACKAGER.WIT_PATH, b"package stravia:platform-plugin@1.0.0;\nworld platform-plugin {}\n")
        self.commit()
        return self.git("rev-parse", "HEAD").decode().strip()

    def source_patch(self, commit):
        with contextlib.redirect_stdout(io.StringIO()):
            return PACKAGER.build_source_patch(self.repo, self.base, commit)

    def test_full_patch_reconstructs_root_configuration_binary_and_maintenance(self):
        head = self.change_source()
        self.write("private/not-source.txt", b"untracked local data must never ship")
        self.write("untracked-source.txt", b"untracked data must never ship")
        index_before = self.git("write-tree")
        patch, proof = self.source_patch(head)
        for path in ["Cargo.toml", "Cargo.lock", "Taskfile.yml", ".cargo/config.toml", ".github/workflows/build.yml", "px/README.md", ".env.integration.example"]:
            self.assertIn(path, proof["changed_paths"])
        self.assertIn("backend/old.rs", proof["changed_paths"])
        self.assertIn("backend/new.rs", proof["changed_paths"])
        self.assertIn(b"GIT binary patch", patch)
        self.assertNotIn(b"untracked data", patch)
        self.assertNotIn(b"untracked local data", patch)
        self.assertEqual(proof["reconstructed_tree"], self.git("rev-parse", "HEAD^{tree}").decode().strip())
        self.assertEqual(proof["source_tree"], proof["reconstructed_tree"])
        self.assertEqual(index_before, self.git("write-tree"))

    def test_previous_backend_frontend_only_patch_cannot_reconstruct_source(self):
        head = self.change_source()
        old_patch = self.git("diff", "--binary", self.base, head, "--", "backend", "frontend")
        with self.assertRaisesRegex(RuntimeError, "complete source commit tree"):
            PACKAGER.verify_source_patch(self.repo, self.base, head, old_patch)

    def test_tracked_local_only_paths_abort_before_patch_is_built(self):
        forbidden = ["private/should-not-ship.txt", "evidence/local.txt", "target/generated.txt", "frontend/node_modules/local.txt", ".env"]
        for path in forbidden:
            self.write(path, b"local test data")
        self.git("add", "--force", "--", *forbidden)
        self.git("commit", "--quiet", "-m", "invalid tracked local data")
        head = self.git("rev-parse", "HEAD").decode().strip()
        with self.assertRaisesRegex(RuntimeError, "Local-only paths") as caught:
            self.source_patch(head)
        for path in forbidden:
            self.assertIn(path, str(caught.exception))

    def test_package_hashes_every_flat_payload_and_preserves_managed_components(self):
        head = self.change_source()
        components = {
            "target/wasm32-wasip2/release/strinova_model_mapping.wasm": b"model mapping",
            "target/release/stravia-server": b"native host fixture",
            "target/vendor-plugins/stravia-vendor-base.wasm": b"base component fixture",
            "target/wasm32-wasip2/release/stravia_vendor_codex.wasm": b"codex component fixture",
            "target/wasm32-wasip2/release/stravia_vendor_antigravity.wasm": b"antigravity component fixture",
            "target/wasm32-wasip2/release/stravia_vendor_claudecode.wasm": b"claude identity component fixture",
            "target/wasm32-wasip2/release/stravia_platform_plugin_example.wasm": b"platform policy/tools fixture",
        }
        for name, content in components.items():
            self.write(name, content)
        self.write("target/vendor-plugins/manifest.json", json.dumps([{
            "vendor_id": "base", "file": "stravia-vendor-base.wasm",
            "sha256": hashlib.sha256(components["target/vendor-plugins/stravia-vendor-base.wasm"]).hexdigest(),
        }]).encode())
        self.write("frontend/stravia-webui/dist/_app/immutable/chunks/app.js", b"immutable asset fixture")
        environment = {
            "GITHUB_SHA": head, "GITHUB_RUN_ID": "123", "GITHUB_SERVER_URL": "https://github.com",
            "GITHUB_REPOSITORY": "example/private-repo",
        }
        out = self.repo / "dist/px-native"
        with contextlib.redirect_stdout(io.StringIO()):
            result = PACKAGER.package_native(
                self.repo, out, environment, base_commit=self.base, rust_version="rustc fixture",
            )
        manifest = json.loads((out / "build-manifest.json").read_text())
        self.assertEqual(manifest["source_commit"], head)
        self.assertEqual(manifest["source_base_commit"], self.base)
        self.assertIn("platform_plugins_v1", manifest["features"])
        self.assertIn("native_custom_tools_v1", manifest["features"])
        self.assertIn("outbound_identity_tls_v1", manifest["features"])
        self.assertEqual(manifest["platform_plugins"]["sdk"]["api_version"], 1)
        self.assertEqual(manifest["platform_plugins"]["wit"]["api_version"], 1)
        self.assertEqual(manifest["platform_plugins"]["wit"]["sha256"], PACKAGER.digest(self.repo / PACKAGER.WIT_PATH))
        self.assertEqual(manifest["platform_plugins"]["example"], {"file": "policy-tools.wasm", **manifest["files"]["policy-tools.wasm"]})
        self.assertEqual(manifest["immutable_asset_count"], 1)
        package = out / result["package"]
        self.assertEqual(result["sha256"], PACKAGER.digest(package))
        with tarfile.open(package) as archive:
            # The manifest cannot contain its own hash; it is covered by SHA256SUMS.
            flat_payload = {member.name for member in archive.getmembers() if member.isfile() and "/" not in member.name and member.name != "build-manifest.json"}
            self.assertEqual(flat_payload, set(manifest["files"]))
            self.assertEqual(flat_payload, {"stravia-server", "base.wasm", "codex.wasm", "antigravity.wasm", "claudecode.wasm", "model-mapping.wasm", "policy-tools.wasm", "source.patch"})
            for name in flat_payload:
                content = archive.extractfile(name).read()
                self.assertEqual(manifest["files"][name], {"sha256": hashlib.sha256(content).hexdigest(), "bytes": len(content)})
        sums = (out / "SHA256SUMS").read_text()
        self.assertIn(PACKAGER.digest(out / "build-manifest.json") + "  build-manifest.json", sums)


if __name__ == "__main__":
    unittest.main()
