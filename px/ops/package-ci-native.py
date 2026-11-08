"""Package CI outputs with provenance and a verified, complete source patch."""

import argparse
import datetime
import hashlib
import json
import os
import pathlib
import re
import subprocess
import tarfile
import tempfile


BASE_COMMIT = "a100ef42031073fbdd779e4809fb83cac3a8b956"
LOCAL_ONLY_ROOTS = {
    ".aws", ".git", ".scratch", ".stravia-dev", "data", "dist", "evidence",
    "node_modules", "private", "target",
}
WIT_PATH = "backend/crates/stravia-platform-plugin-sdk/wit/platform-plugin.wit"


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            value.update(chunk)
    return value.hexdigest()


def git(repo, *arguments, input=None, environment=None):
    return subprocess.check_output(
        ["git", "-c", "safe.directory=" + repo.resolve().as_posix(), "-C", str(repo), *arguments],
        input=input,
        env=environment,
    )


def verify_source_patch(repo, base_commit, commit, patch):
    """Apply to an alternate index, preserving the checkout and real index."""
    base_tree = git(repo, "rev-parse", base_commit + "^{tree}").decode().strip()
    source_tree = git(repo, "rev-parse", commit + "^{tree}").decode().strip()
    with tempfile.TemporaryDirectory(prefix="stravia-source-index-") as directory:
        environment = {**os.environ, "GIT_INDEX_FILE": str(pathlib.Path(directory) / "index")}
        git(repo, "read-tree", base_tree, environment=environment)
        if patch:
            git(repo, "apply", "--cached", "--check", input=patch, environment=environment)
            git(repo, "apply", "--cached", input=patch, environment=environment)
        reconstructed_tree = git(repo, "write-tree", environment=environment).decode().strip()
    if reconstructed_tree != source_tree:
        raise RuntimeError("Source patch does not reconstruct the complete source commit tree")
    return {
        "base_tree": base_tree,
        "source_tree": source_tree,
        "reconstructed_tree": reconstructed_tree,
    }


def build_source_patch(repo, base_commit, commit):
    """Include every tracked change, including root build files and maintenance."""
    paths = git(
        repo, "diff-tree", "--no-commit-id", "--name-only", "--no-renames", "-r", "-z",
        base_commit, commit,
    ).decode("utf-8").split("\0")
    paths = [path for path in paths if path]
    forbidden = []
    for path in paths:
        parts = pathlib.PurePosixPath(path).parts
        local_environment = parts[0] == ".env" or (parts[0].startswith(".env.") and not parts[0].endswith(".example"))
        if parts[0] in LOCAL_ONLY_ROOTS or local_environment or "node_modules" in parts or "target" in parts:
            forbidden.append(path)
    if forbidden:
        raise RuntimeError("Local-only paths are tracked in the source change range: " + ", ".join(forbidden))
    print(json.dumps({"source_changed_paths": paths}))
    patch = git(
        repo, "diff", "--binary", "--full-index", "--no-ext-diff", "--no-textconv",
        "--no-renames", "--no-color", "--src-prefix=a/", "--dst-prefix=b/", base_commit, commit,
    )
    validation = verify_source_patch(repo, base_commit, commit, patch)
    return patch, {**validation, "changed_paths": paths}


def package_native(repo, out, environment, *, base_commit=BASE_COMMIT, rust_version=None):
    commit = git(repo, "rev-parse", "HEAD").decode().strip()
    if commit != environment["GITHUB_SHA"]:
        raise RuntimeError("Package commit differs from workflow commit")
    if git(repo, "status", "--porcelain", "--untracked-files=no").strip():
        raise RuntimeError("Tracked source files must match the workflow commit before packaging")
    version_match = re.search(r'(?m)^version = "([0-9.]+)"$', (repo / "Cargo.toml").read_text(encoding="utf-8"))
    if version_match is None:
        raise RuntimeError("Workspace version is missing")
    version = version_match.group(1)
    patch, source_validation = build_source_patch(repo, base_commit, commit)
    out.mkdir(parents=True, exist_ok=True)
    (out / "source.patch").write_bytes(patch)

    base = next(
        item for item in json.loads((repo / "target/vendor-plugins/manifest.json").read_text(encoding="utf-8"))
        if item["vendor_id"] == "base"
    )
    if pathlib.PurePosixPath(base["file"]).name != base["file"]:
        raise RuntimeError("Base component filename must be flat")
    components = {
        "base.wasm": repo / "target/vendor-plugins" / base["file"],
        "codex.wasm": repo / "target/wasm32-wasip2/release/stravia_vendor_codex.wasm",
        "antigravity.wasm": repo / "target/wasm32-wasip2/release/stravia_vendor_antigravity.wasm",
        "policy-tools.wasm": repo / "target/wasm32-wasip2/release/stravia_platform_plugin_example.wasm",
    }
    files = {
        "stravia-server": repo / "target/release/stravia-server",
        **components,
        "source.patch": out / "source.patch",
    }
    if digest(components["base.wasm"]) != base["sha256"]:
        raise RuntimeError("Base component checksum differs from its build manifest")
    file_manifest = {
        name: {"sha256": digest(path), "bytes": path.stat().st_size}
        for name, path in files.items()
    }
    manifest = {
        "upstream_tag": "v" + version,
        "upstream_commit": base_commit,
        "source_base_commit": base_commit,
        "source_commit": commit,
        "features": [
            "positional_additional_tools", "per_key_request_model_mapping_v1",
            "native_custom_tools_v1", "platform_plugins_v1",
        ],
        "build_target": "x86_64-unknown-linux-gnu",
        "glibc_baseline": "2.35",
        "rust_version": rust_version or subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "built_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "github_run_id": environment["GITHUB_RUN_ID"],
        "github_run_url": environment["GITHUB_SERVER_URL"] + "/" + environment["GITHUB_REPOSITORY"]
        + "/actions/runs/" + environment["GITHUB_RUN_ID"],
        "source_patch_validation": source_validation,
        "platform_plugins": {
            "api_version": 1,
            "sdk": {"crate": "stravia-platform-plugin-sdk", "version": version, "api_version": 1},
            "wit": {
                "package": "stravia:platform-plugin@1.0.0", "world": "platform-plugin",
                "api_version": 1, "sha256": digest(repo / WIT_PATH),
            },
            "example": {"file": "policy-tools.wasm", **file_manifest["policy-tools.wasm"]},
        },
        "files": file_manifest,
    }
    immutable = repo / "frontend/stravia-webui/dist/_app/immutable"
    manifest["immutable_asset_count"] = sum(path.is_file() for path in immutable.rglob("*"))
    if manifest["immutable_asset_count"] == 0:
        raise RuntimeError("The WebUI immutable assets are missing")
    manifest_path = out / "build-manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    package = out / ("stravia-px-v" + version + "-linux-x86_64-gnu.tar.gz")
    with tarfile.open(package, "w:gz", compresslevel=6) as archive:
        for name, path in files.items():
            archive.add(path, arcname=name, recursive=False)
        archive.add(manifest_path, arcname="build-manifest.json", recursive=False)
        archive.add(immutable, arcname="immutable")
    (out / "SHA256SUMS").write_text("".join(
        digest(path) + "  " + path.name + "\n"
        for path in [package, manifest_path, out / "source.patch"]
    ), encoding="utf-8")
    return {
        "source_commit": commit, "package": package.name,
        "sha256": digest(package), "bytes": package.stat().st_size,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify-source-only", action="store_true", help="Verify tracked source before compiling")
    arguments = parser.parse_args()
    repo = pathlib.Path(__file__).resolve().parents[2]
    if arguments.verify_source_only:
        commit = git(repo, "rev-parse", "HEAD").decode().strip()
        if commit != os.environ["GITHUB_SHA"]:
            raise RuntimeError("Source commit differs from workflow commit")
        _, validation = build_source_patch(repo, BASE_COMMIT, commit)
        print(json.dumps({"source_commit": commit, "source_patch_validation": validation}))
    else:
        print(json.dumps(package_native(repo, repo / "dist/px-native", os.environ)))


if __name__ == "__main__":
    main()
