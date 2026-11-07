"""Package native host plus immutable assets, or the isolated Linux test binary."""
import argparse,hashlib,json,pathlib,re,tarfile
p=argparse.ArgumentParser();p.add_argument('--tests',action='store_true');p.add_argument('--repository',type=pathlib.Path);p.add_argument('--output-directory',type=pathlib.Path);args=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[1]
repo=args.repository or (root/'stravia-compat' if (root/'stravia-compat/Cargo.toml').is_file() else root.parent)
private=args.output_directory or (root/'private' if (root/'stravia-compat/Cargo.toml').is_file() else None)
if private is None:p.error('--output-directory must name an ignored or external artifact directory')
private.mkdir(parents=True,exist_ok=True)
def digest(path):
 h=hashlib.sha256()
 with path.open('rb') as stream:
  while data:=stream.read(1024*1024):h.update(data)
 return h.hexdigest()
if args.tests:
 candidates=[p for p in (repo/'target/x86_64-unknown-linux-gnu/release/deps').glob('stravia_core-*') if p.is_file() and re.fullmatch(r'stravia_core-[0-9a-f]+',p.name)]
 assert candidates,'Linux test binary is not built'
 binary=max(candidates,key=lambda p:p.stat().st_mtime);package=private/'stravia-request-model-tests.tar.gz'
 with tarfile.open(package,'w:gz',compresslevel=6) as archive:archive.add(binary,arcname='stravia-core-tests',recursive=False)
 report={'binary_sha256':digest(binary),'binary_bytes':binary.stat().st_size,'package_sha256':digest(package),'package_bytes':package.stat().st_size,'binary_source':str(binary)}
 (private/'stravia-request-model-test-build.json').write_text(json.dumps(report,indent=2));print(json.dumps(report))
else:
 binary=repo/'target/x86_64-unknown-linux-gnu/release/stravia-server';assets=repo/'frontend/stravia-webui/dist/_app/immutable'
 assert binary.is_file() and assets.is_dir()
 package=private/'stravia-request-model-host.tar.gz'
 with tarfile.open(package,'w:gz',compresslevel=6) as archive:
  archive.add(binary,arcname='stravia-server',recursive=False)
  archive.add(assets,arcname='immutable')
 report={'host_sha256':digest(binary),'host_bytes':binary.stat().st_size,'package_sha256':digest(package),'package_bytes':package.stat().st_size,'immutable_asset_count':sum(p.is_file() for p in assets.rglob('*'))}
 (private/'stravia-request-model-host-build.json').write_text(json.dumps(report,indent=2));print(json.dumps(report))
