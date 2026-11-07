"""Package CI outputs with provenance and all managed compatibility components."""
import datetime,hashlib,json,os,pathlib,shutil,subprocess,tarfile,re
repo=pathlib.Path(__file__).resolve().parents[2];out=repo/'dist/px-native';out.mkdir(parents=True,exist_ok=True)
def digest(path):
 h=hashlib.sha256()
 with path.open('rb') as stream:
  while chunk:=stream.read(1024*1024):h.update(chunk)
 return h.hexdigest()
commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()
assert commit==os.environ['GITHUB_SHA'],'Package commit differs from workflow commit'
version=re.search(r'(?m)^version = "([0-9.]+)"$',(repo/'Cargo.toml').read_text()).group(1)
base_commit='a100ef42031073fbdd779e4809fb83cac3a8b956'
patch=subprocess.check_output(['git','diff','--binary',base_commit,commit,'--','backend','frontend'],cwd=repo)
(out/'source.patch').write_bytes(patch)
base=json.loads((repo/'target/vendor-plugins/manifest.json').read_text())
base=next(x for x in base if x['vendor_id']=='base')
components={
 'base.wasm':repo/'target/vendor-plugins'/base['file'],
 'codex.wasm':repo/'target/wasm32-wasip2/release/stravia_vendor_codex.wasm',
 'antigravity.wasm':repo/'target/wasm32-wasip2/release/stravia_vendor_antigravity.wasm',
}
files={'stravia-server':repo/'target/release/stravia-server',**components,'source.patch':out/'source.patch'}
assert digest(components['base.wasm'])==base['sha256']
manifest={
 'upstream_tag':'v'+version,'upstream_commit':base_commit,'source_base_commit':base_commit,'source_commit':commit,
 'features':['positional_additional_tools','per_key_request_model_mapping_v1','native_custom_tools_v1'],
 'build_target':'x86_64-unknown-linux-gnu','glibc_baseline':'2.35','rust_version':subprocess.check_output(['rustc','--version'],text=True).strip(),
 'built_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'github_run_id':os.environ['GITHUB_RUN_ID'],
 'github_run_url':os.environ['GITHUB_SERVER_URL']+'/'+os.environ['GITHUB_REPOSITORY']+'/actions/runs/'+os.environ['GITHUB_RUN_ID'],
 'files':{name:{'sha256':digest(path),'bytes':path.stat().st_size} for name,path in files.items()},
}
manifest['immutable_asset_count']=sum(p.is_file() for p in (repo/'frontend/stravia-webui/dist/_app/immutable').rglob('*'))
assert manifest['immutable_asset_count']>0
(out/'build-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
package=out/('stravia-px-v'+version+'-linux-x86_64-gnu.tar.gz')
with tarfile.open(package,'w:gz',compresslevel=6) as archive:
 for name,path in files.items():archive.add(path,arcname=name,recursive=False)
 archive.add(out/'build-manifest.json',arcname='build-manifest.json',recursive=False)
 archive.add(repo/'frontend/stravia-webui/dist/_app/immutable',arcname='immutable')
(out/'SHA256SUMS').write_text(''.join(digest(p)+'  '+p.name+'\n' for p in [package,out/'build-manifest.json',out/'source.patch']))
print(json.dumps({'source_commit':commit,'package':package.name,'sha256':digest(package),'bytes':package.stat().st_size}))
