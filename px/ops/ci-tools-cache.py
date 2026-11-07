"""Record or verify compiled tooling before reusing an exact-source CI cache."""
import argparse,hashlib,json,pathlib
p=argparse.ArgumentParser();p.add_argument('action',choices=['record','verify']);args=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[2];record=root/'target/px-tools-cache.json'
def digest(path):
 h=hashlib.sha256()
 with path.open('rb') as stream:
  while data:=stream.read(1024*1024):h.update(data)
 return h.hexdigest()
required=[root/'target/debug/stravia-build-vendors',root/'target/vendor-plugins/manifest.json',root/'target/wasm32-wasip2/release/stravia_vendor_codex.wasm',root/'target/wasm32-wasip2/release/stravia_vendor_antigravity.wasm']
manifest=json.loads(required[1].read_text())
for component in manifest:
 filename=component['file'];assert pathlib.Path(filename).name==filename,'Invalid cached component path'
 path=required[1].parent/filename;assert digest(path)==component['sha256'],'Cached component differs from its manifest';required.append(path)
expected={str(path.relative_to(root).as_posix()):{'sha256':digest(path),'bytes':path.stat().st_size} for path in required}
if args.action=='record':record.write_text(json.dumps({'schema':1,'files':expected},indent=2)+'\n')
else:
 cached=json.loads(record.read_text());assert cached=={'schema':1,'files':expected},'Compiled tool cache checksum mismatch'
print(json.dumps({'cache_action':args.action,'verified_files':len(expected),'contains_production_credentials':False}))
